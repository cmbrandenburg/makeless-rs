use std;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct RuleSet {
    inner: HashMap<PathBuf, Rule>,
}

// A dependency chain is a cyclic sequence of target paths. This means that, for
// example, A->B->C->A, B->C->A->B, and C->A->B->C are the same dependency chain.
//
// Invariant: For any dependency sequence (A, B, ..., C), A <= B, ... A <= C.
// This ensures that derived equality-comparison and hashing behave as expected.
//
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DependencyChain<'a>(Vec<&'a Path>);

impl<'a> DependencyChain<'a> {
    fn new<I>(targets: I) -> Self
        where I: IntoIterator<Item = &'a Path>
    {
        let targets = targets.into_iter().collect::<Vec<_>>();
        debug_assert!(!targets.is_empty());

        // Need to find the smallest target value and rotate the vector so that
        // it's the first element. E.g., [C, D, A, B] -> [A, B, C, D].
        let index_smallest = targets.iter()
            .enumerate()
            .min_by(|&(_i, &ipath), &(_j, &jpath)| {
                if ipath < jpath {
                    std::cmp::Ordering::Less
                } else if ipath == jpath {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Greater
                }
            })
            .unwrap()
            .0;

        let targets = {
            let mut v = Vec::new();
            v.extend_from_slice(&targets[index_smallest..]);
            v.extend_from_slice(&targets[..index_smallest]);
            v
        };

        DependencyChain(targets)
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct DependencyScan<'a> {
    cyclic_dependencies: HashSet<DependencyChain<'a>>,
    missing_dependencies: HashSet<&'a Path>,
}

impl RuleSet {
    pub fn new() -> Self {
        RuleSet { inner: HashMap::new() }
    }

    pub fn insert(&mut self, rule: Rule) -> Option<Rule> {
        self.inner.insert(rule.target.clone(), rule)
    }

    pub fn scan_dependencies(&self) -> DependencyScan {

        fn recurse<'a>(dot: &'a Rule,
                       rule_set: &'a RuleSet,
                       mut dep_path: Vec<&'a Path>,
                       scan: &mut DependencyScan<'a>) {

            // Need to pare dependency cycles to the shortest path containing
            // the cycle. For example, the cycle in A->B->C->B is B->C, not
            // A->B->C.

            let dot_target = &dot.target;
            if let Some((index, _)) = dep_path.iter().enumerate().find(|&(_index, &x)| x == dot_target) {
                scan.cyclic_dependencies.insert(DependencyChain::new(dep_path[index..].iter().map(|&x| x)));
                return;
            }

            dep_path.push(dot_target);
            for t in &dot.dependencies {
                match rule_set.inner.get(t) {
                    None => {
                        scan.missing_dependencies.insert(t);
                    }
                    Some(next_dot) => {
                        recurse(next_dot, rule_set, dep_path.clone(), scan);
                    }
                }
            }
        }

        let mut scan = DependencyScan::default();
        for (_target, rule) in &self.inner {
            recurse(rule, self, vec![], &mut scan);
        }

        // Need to delete redundant cycles. For example, we can have A->B->C and
        // B->C, in which case we should delete A->B->C.

        scan.cyclic_dependencies = {
            let orig_cycles =
                scan.cyclic_dependencies.iter().map(|&DependencyChain(ref path)| &path[..]).collect::<HashSet<_>>();
            let mut unique_cycles = HashSet::new();
            for &path in &orig_cycles {
                let mut nok = false;
                for i in 1..path.len() {
                    if orig_cycles.get(&path[i..]).is_some() {
                        nok = true;
                        break;
                    }
                }
                if !nok {
                    unique_cycles.insert(path.clone());
                }
            }
            unique_cycles.iter().map(|&x| DependencyChain(Vec::from(x))).collect::<HashSet<_>>()
        };

        scan
    }
}

pub struct Rule {
    target: PathBuf,
    recipe: Option<Box<FnOnce() -> Result<(), Box<std::error::Error>>>>,
    phony: bool,
    dependencies: HashSet<PathBuf>,
}

impl Rule {
    pub fn new<P: Into<PathBuf>>(target: P) -> Self {
        Rule {
            target: target.into(),
            recipe: None,
            phony: false,
            dependencies: HashSet::new(),
        }
    }

    pub fn with_recipe<E, F>(mut self, recipe: F) -> Self
        where E: 'static + std::error::Error,
              F: 'static + FnOnce() -> Result<(), E>
    {
        let f = || recipe().map_err(|e| -> Box<std::error::Error> { Box::new(e) });
        self.recipe = Some(Box::new(f));
        self
    }

    pub fn with_phony(mut self, phony: bool) -> Self {
        self.phony = phony;
        self
    }

    pub fn with_dependency<P: Into<PathBuf>>(mut self, dependency: P) -> Self {
        self.dependencies.insert(dependency.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use super::*;

    struct RuleSetBuilder(RuleSet);

    impl RuleSetBuilder {
        fn new() -> Self {
            RuleSetBuilder(RuleSet::new())
        }

        fn with_rule(mut self, rule: Rule) -> Self {
            self.0.insert(rule);
            self
        }

        fn build(self) -> RuleSet {
            self.0
        }
    }

    #[test]
    fn scan_dependencies() {

        #[derive(Debug)]
        struct Expected<'a>(DependencyScan<'a>);

        impl<'a> Expected<'a> {
            fn new() -> Self {
                Expected(DependencyScan::default())
            }

            fn with_cyclic_dependency<I>(mut self, targets: I) -> Self
                where I: IntoIterator<Item = &'static str>
            {
                let targets = targets.into_iter().map(|x| Path::new(x));
                self.0.cyclic_dependencies.insert(DependencyChain::new(targets));
                self
            }

            fn with_missing_dependency(mut self, target: &'static str) -> Self {
                self.0.missing_dependencies.insert(Path::new(target));
                self
            }
        }

        impl<'a> PartialEq<DependencyScan<'a>> for Expected<'a> {
            fn eq(&self, other: &DependencyScan<'a>) -> bool {
                self.0.eq(other)
            }
        }

        // {} -> (), ()

        {
            let rule_set = RuleSetBuilder::new().build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->()} -> (), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A"))
                .build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(A)} -> (A->A), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("A"))
                .build();
            let expected = Expected::new().with_cyclic_dependency(vec!["A"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B)} -> (), (B)

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .build();
            let expected = Expected::new().with_missing_dependency("B");
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B, C, D)} -> (), (B, C, D)

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B").with_dependency("C").with_dependency("D"))
                .build();
            let expected =
                Expected::new().with_missing_dependency("B").with_missing_dependency("C").with_missing_dependency("D");
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(C)} -> (), (C)

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .build();
            let expected = Expected::new().with_missing_dependency("C");
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->()} -> (), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B"))
                .build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(C), B->(C), C->()} -> (), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("C"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C"))
                .build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(C), C->(D), D->()} -> (), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C").with_dependency("D"))
                .with_rule(Rule::new("D"))
                .build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(C), C->(D)} -> (), (D)

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C").with_dependency("D"))
                .build();
            let expected = Expected::new().with_missing_dependency("D");
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(), B->()} -> (), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A"))
                .with_rule(Rule::new("B"))
                .build();
            let expected = Expected::new();
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(A)} -> (A->B), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("A"))
                .build();
            let expected = Expected::new().with_cyclic_dependency(vec!["A", "B"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(C), C->(A)} -> (A->C), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C").with_dependency("A"))
                .build();
            let expected = Expected::new().with_cyclic_dependency(vec!["A", "B", "C"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B), B->(C), C->(B)} -> (B->C), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C").with_dependency("B"))
                .build();
            let expected = Expected::new().with_cyclic_dependency(vec!["B", "C"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B, C), B->(A), C->(A)} -> (A->B, A->C), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B").with_dependency("C"))
                .with_rule(Rule::new("B").with_dependency("A"))
                .with_rule(Rule::new("C").with_dependency("A"))
                .build();
            let expected =
                Expected::new().with_cyclic_dependency(vec!["A", "B"]).with_cyclic_dependency(vec!["A", "C"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }

        // {A->(B, C), B->(C), C->(A, B)} -> (A->C, B->C), ()

        {
            let rule_set = RuleSetBuilder::new()
                .with_rule(Rule::new("A").with_dependency("B").with_dependency("C"))
                .with_rule(Rule::new("B").with_dependency("C"))
                .with_rule(Rule::new("C").with_dependency("A").with_dependency("B"))
                .with_rule(Rule::new("D"))
                .build();
            let expected =
                Expected::new().with_cyclic_dependency(vec!["A", "C"]).with_cyclic_dependency(vec!["B", "C"]);
            let got = rule_set.scan_dependencies();
            assert_eq!(expected, got);
        }
    }
}
