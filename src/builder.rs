// pub struct Builder {
//     rules: HashMap<PathBuf, Rule>,
//     default_target: Option<PathBuf>,
// }
//
// impl Builder {
//     pub fn new() -> Self {
//         Builder {
//             rules: HashMap::new(),
//             default_target: None,
//         }
//     }
//
//     pub fn with_rule(mut self, rule: Rule) -> Self {
//
//         // The default target is, by default, the first rule inserted.
//         if self.default_target.is_none() {
//             self.default_target = Some(rule.target.clone());
//         }
//
//         self.rules.insert(rule.target.clone(), rule);
//         self
//     }
// }
