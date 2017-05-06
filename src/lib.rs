extern crate futures;
extern crate num_cpus;
extern crate petgraph;

mod error;

pub use error::Error;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
pub struct ContextHandle {
    context: Context,
}

#[derive(Clone, Debug)]
pub struct RuleHandle {
    context: Context,
    rule_id: RuleId,
}

type Context = Arc<Mutex<ContextBlock>>;

#[derive(Debug)]
struct ContextBlock {
    all_rules: petgraph::stable_graph::StableDiGraph<Rule, ()>,
    all_targets: HashSet<PathBuf>,
    default_rule_id: Option<RuleId>,

    // thread pool:
    threads: Vec<std::thread::JoinHandle<()>>,
}

#[derive(Debug)]
struct Rule {
    target: PathBuf,
    status: RuleStatus,
    recipe: Option<Recipe>,
}

type RuleId = petgraph::graph::NodeIndex<u32>;

#[derive(Debug)]
pub struct RunFuture {
    inner: RunFutureInner,
}

impl ContextHandle {
    pub fn new() -> Self {
        ContextHandle { context: Arc::new(Mutex::new(ContextBlock::new())) }
    }

    pub fn new_rule<P>(&self, target: P) -> Result<RuleHandle, Error>
        where P: Into<PathBuf>
    {
        let rule_id = {
            let mut context_block = ContextBlock::lock(&self.context)?;
            context_block.new_rule(target.into())?
        };

        Ok(RuleHandle {
               context: self.context.clone(),
               rule_id: rule_id,
           })
    }
}

impl RuleHandle {
    pub fn run(&self) -> RunFuture {

        let mut context_block = match ContextBlock::lock(&self.context) {
            Ok(x) => x,
            Err(e) => return RunFuture::new_failed(e),
        };

        match context_block.run_rule(self.rule_id) {
            Ok(..) => RunFuture::new(self.clone()),
            Err(e) => RunFuture::new_failed(e),
        }
    }

    pub fn set_recipe<E, R>(&self, recipe: R) -> Result<(), Error>
        where E: 'static + std::error::Error,
              R: 'static + FnOnce() -> Result<(), E> + Send
    {
        // The *inner closure* is a workaround for dealing with stable Rust's
        // Box+FnOnce limitation, combined with allowing the application to
        // return any error type to denote failure. Basically, the inner closure
        // promotes the application's FnOnce to a boxed FnMut, and it boxes any
        // error that's returned. It works so long as we don't call the inner
        // closure twice--despite it using FnMut instead of FnOnce.

        let mut inner_closure = Some(recipe);
        let outer_closure = move || {
            let f = inner_closure.take().unwrap();
            f().map_err(|error| -> Error { Error::RecipeFailure(error::RecipeFailureDetails::new_from_error(error)) })
        };

        self.set_recipe_impl(Recipe(Box::new(outer_closure)))
    }

    // The `set_recipe_of_any_error` function is a workaround for the
    // `set_recipe` method being unable to accept any error type in the callback
    // while still being able to extract the error's display string. Perhaps in
    // the future we can merge both of these methods into one.

    pub fn set_recipe_of_any_error<E, R>(&self, recipe: R) -> Result<(), Error>
        where E: 'static,
              R: 'static + FnOnce() -> Result<(), E> + Send
    {
        let mut inner_closure = Some(recipe);
        let outer_closure = move || {
            let f = inner_closure.take().unwrap();
            f().map_err(|any| -> Error { Error::RecipeFailure(error::RecipeFailureDetails::new_from_any(any)) })
        };

        self.set_recipe_impl(Recipe(Box::new(outer_closure)))
    }

    fn set_recipe_impl(&self, recipe: Recipe) -> Result<(), Error> {

        let mut context_block = ContextBlock::lock(&self.context)?;

        context_block
            .all_rules
            .node_weight_mut(self.rule_id)
            .unwrap()
            .recipe = Some(recipe);

        Ok(())
    }

    pub fn new_dependency<P>(&mut self, target: P) -> Result<RuleHandle, Error>
        where P: Into<PathBuf>
    {
        let rule_id = {
            let mut context_block = ContextBlock::lock(&self.context)?;
            let new_rule_id = context_block.new_rule(target.into())?;

            context_block
                .all_rules
                .add_edge(self.rule_id, new_rule_id, ());

            context_block.set_unknown_recursively(self.rule_id);

            new_rule_id
        };

        Ok(RuleHandle {
               context: self.context.clone(),
               rule_id: rule_id,
           })
    }
}

impl ContextBlock {
    fn new() -> Self {
        ContextBlock {
            all_rules: petgraph::stable_graph::StableDiGraph::new(),
            all_targets: HashSet::new(),
            default_rule_id: None,
            threads: Vec::new(),
        }
    }

    fn lock(context: &Context) -> Result<MutexGuard<ContextBlock>, Error> {
        context
            .lock()
            .map_err(|e| Error::RawMessage { message: format!("Context mutex is poisoned: {}", e) })
    }

    fn new_rule(&mut self, target: PathBuf) -> Result<RuleId, Error> {

        // There cannot be two rules with the same target in a context.

        if self.all_targets.contains(&target) {
            return Err(Error::RawMessage { message: format!("Target {:?} already exists in the context", target) });
        }

        // All checks pass. Construct a new rule and add it to this context.

        let rule_block = Rule {
            target: target.clone(),
            status: RuleStatus::Unknown,
            recipe: None,
        };

        let rule_id = self.all_rules.add_node(rule_block);
        self.all_targets.insert(target);

        if self.default_rule_id.is_none() {
            self.default_rule_id = Some(rule_id);
        }

        Ok(rule_id)
    }

    fn set_unknown_recursively(&mut self, rule_id: RuleId) {

        // TODO: This function is ripe for optimization.

        {
            let rule = self.all_rules.node_weight(rule_id).unwrap();
            if rule.status.is_unknown() {
                return;
            }
        }

        let dependents = self.all_rules
            .neighbors_directed(rule_id, petgraph::Direction::Incoming)
            .collect::<Vec<_>>();

        for dependent_rule_id in dependents {
            self.set_unknown_recursively(dependent_rule_id);
        }
    }

    fn run_rule(&mut self, rule_id: RuleId) -> Result<(), Error> {
        self.prep_rule_tree_statuses(rule_id)?;
        self.exec_rule(rule_id)
    }

    fn prep_rule_tree_statuses(&mut self, root_rule_id: RuleId) -> Result<(), Error> {

        let mut rule = self.all_rules.node_weight_mut(root_rule_id).unwrap();
        if !rule.status.is_unknown() {
            return Ok(());
        }

        let dependency_rule_ids = self.all_rules
            .neighbors_directed(root_rule_id, petgraph::Directed::Outgoing)
            .collect::<Vec<_>>();

        let mut wait_count = 0;

        for r in dependency_rule_ids {
            self.prep_rule_tree_statuses(self, r);
            match self.all_rules.node_weight(r).unwrap().status {
                RuleStatus::Unknown => unreachable!(),
                RuleStatus::UpToDate => {}
                RuleStatus::OutOfDate { .. } => {
                    wait_count += 1;
                }
                RuleStatus::InProgress => {
                    wait_count += 1;
                }
                RuleStatus::Failed (

            }
        }


        unimplemented!();
    }

    fn exec_rule(&mut self, rule_id: RuleId) -> Result<(), Error> {

        // To-do
        //
        // * If this rule's status is 'unknown' then determine whether it is
        //   (and its dependencies are) 'up-to-date' or 'out-of-date'.
        //
        // * Match on status:
        //
        //   * Up-to-date: We're done.
        //
        //   * Out-of-date: Recursively execute any dependencies that are
        //     out-of-date.


        // let mut rule = self.all_rules.node_weight_mut(rule_id).unwrap();

        // if rule.status.is_unknown() {
        //     for dependent_rule_id in
        //         self.all_rules
        //             .neighbors_directed(rule_id, petgraph::Direction::Outgoing) {}
        // }

        unimplemented!();
    }
}

impl RunFuture {
    fn new(rule_handle: RuleHandle) -> Self {
        RunFuture { inner: RunFutureInner::Normal(rule_handle) }
    }

    fn new_failed(error: Error) -> Self {
        RunFuture { inner: RunFutureInner::NoRuleBlock(error) }
    }
}

impl futures::Future for RunFuture {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        unimplemented!();
    }
}

// The rationale for the Recipe type is that it implements Debug, which
// Box<FnMut> cannot derive.
struct Recipe(Box<FnMut() -> Result<(), Error> + Send>);

impl std::fmt::Debug for Recipe {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        "recipe (no debug info)".fmt(f)
    }
}

#[derive(Debug)]
enum RuleStatus {
    // (1) the rule has not run and has not checked whether its target is
    // up-to-date or (2) the rule has had a dependency added that is itself in
    // the Unknown state.
    Unknown,

    // The rule has not run, but its target is out-of-date and must
    // run.
    OutOfDate {
        // Number of dependencies that are out-of-date and must run before this
        // rule can run.
        dependency_wait_count: usize,
    },

    // The rule is currently running.
    InProgress,

    // The rule's target is up-to-date, therefore the rule does not need to run.
    // Perhaps the rule already ran, perhaps its target was already up-to-date
    // since the beginning.
    UpToDate,

    // The rule ran and either failed or is propagating an error from a
    // dependency.
    Failed { error: Error },
}

impl RuleStatus {
    fn is_unknown(&self) -> bool {
        match *self {
            RuleStatus::Unknown => true,
            _ => false,
        }
    }
}

// The RunFutureInner is needed because of a chicken-and-egg problem with
// (1) locking the Rule mutex and (2) reporting errors. Normally, errors
// are stored *in* the Rule, but the Rule must first be unlocked to
// store an error. Hence, if the error is due to failing to unlock the Rule
// then we save the error outright in the `NoRuleBlock` variant, below.
#[derive(Debug)]
enum RunFutureInner {
    NoRuleBlock(Error),
    Normal(RuleHandle),
}
