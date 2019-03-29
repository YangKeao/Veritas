use std::sync::RwLock;
use veritas::*;

struct WrongStack {
    data: RwLock<Vec<u32>>,
}
#[derive(Copy, Clone)]
struct StackAction {}
#[derive(Copy, Clone)]
enum StackRequest {
    PushAction(PushAction),
    #[allow(dead_code)]
    PopAction,
}
#[derive(Copy, Clone)]
struct PushAction {
    data: u32,
}
impl Action for StackAction {
    type Request = StackRequest;
    type Response = u32;
}
impl WrongStack {
    fn new(v: Vec<u32>) -> Self {
        Self {
            data: RwLock::new(v),
        }
    }
    fn push(&self, data: u32) -> u32 {
        self.data.write().unwrap().push(data);
        return data;
    }
}

impl Runner<StackAction> for WrongStack {
    fn init() -> Self {
        WrongStack::new(Vec::new())
    }
    fn invoke(
        &self,
        action: <StackAction as Action>::Request,
    ) -> <StackAction as Action>::Response {
        match action {
            StackRequest::PushAction(push_action) => self.push(push_action.data),
            StackRequest::PopAction => {
                100 // Return 100 constantly
            }
        }
    }
}

struct Stack {
    data: RwLock<Vec<u32>>,
}
impl Stack {
    fn new(v: Vec<u32>) -> Self {
        Self {
            data: RwLock::new(v),
        }
    }
    fn push(&self, data: u32) -> u32 {
        self.data.write().unwrap().push(data);
        return data;
    }
    fn pop(&self) -> u32 {
        self.data.write().unwrap().pop().unwrap()
    }
}
impl Runner<StackAction> for Stack {
    fn init() -> Self {
        Stack::new(Vec::new())
    }
    fn invoke(
        &self,
        action: <StackAction as Action>::Request,
    ) -> <StackAction as Action>::Response {
        match action {
            StackRequest::PushAction(push_action) => self.push(push_action.data),
            StackRequest::PopAction => self.pop(),
        }
    }
}
impl Model<StackAction, Vec<u32>> for Stack {
    fn get_state(&self) -> Vec<u32> {
        self.data.read().unwrap().clone()
    }

    fn from_state(state: &Vec<u32>) -> Self {
        Stack::new(state.clone())
    }
}

#[test]
fn single_thread_stack_test() {
    let mut checker: Checker<StackAction, Vec<u32>, Stack, WrongStack> = Checker::new();
    let mut history = Vec::new();
    for i in 0..100 {
        history.push(StackRequest::PushAction(PushAction { data: i }));
        history.push(StackRequest::PopAction);
    }
    checker.add_thread(history);
    checker.finish_prepare();

    assert_eq!(checker.check(vec![], 4), false);
}
