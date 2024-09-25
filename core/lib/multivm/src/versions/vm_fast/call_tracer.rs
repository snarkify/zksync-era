use zksync_types::zk_evm_types::FarCallOpcode;
use zksync_vm2::interface::{CallframeInterface, Opcode, OpcodeType, StateInterface, Tracer};
use zksync_vm_interface::Call;

#[derive(Debug, Clone, Default)]
pub struct CallTracer {
    stack: Vec<FarcallAndNearCallCount>,

    current_stack_depth: usize,
    max_stack_depth: usize,

    max_near_calls: usize,
}

#[derive(Debug, Clone)]
struct FarcallAndNearCallCount {
    farcall: Call,
    near_calls_after: usize,
}

impl CallTracer {
    pub fn result(&self) -> Vec<Call> {
        self.stack.iter().map(|x| x.farcall.clone()).collect()
    }
}

impl Tracer for CallTracer {
    fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
        match OP::VALUE {
            Opcode::FarCall(tipe) => {
                self.current_stack_depth += 1;
                self.max_stack_depth = self.max_stack_depth.max(self.current_stack_depth);

                self.stack.push(FarcallAndNearCallCount {
                    farcall: Call {
                        r#type: /*match tipe {
                            zksync_vm2::zksync_vm2_interface::CallingMode::Normal => {*/
                                zksync_vm_interface::CallType::Call(FarCallOpcode::Normal)
                        ,
                        ..Default::default()
                    },
                    near_calls_after: 0,
                });
            }
            Opcode::NearCall => {
                self.current_stack_depth += 1;
                self.max_stack_depth = self.max_stack_depth.max(self.current_stack_depth);

                if let Some(frame) = self.stack.last_mut() {
                    frame.near_calls_after += 1;
                    self.max_near_calls = self.max_near_calls.max(frame.near_calls_after);
                }
            }
            Opcode::Ret(_) => {
                self.current_stack_depth -= 1;

                let Some(mut current_call) = self.stack.pop() else {
                    return;
                };

                if current_call.near_calls_after == 0 {
                    current_call.farcall.gas_used = current_call
                        .farcall
                        .parent_gas
                        .saturating_sub(state.current_frame().gas() as u64);

                    // TODO save return value

                    // If there is a parent call, push the current call to it
                    // Otherwise, put the current call back on the stack, because it's the top level call
                    if let Some(parent_call) = self.stack.last_mut() {
                        parent_call.farcall.calls.push(current_call.farcall);
                    } else {
                        self.stack.push(current_call);
                    }
                } else {
                    current_call.near_calls_after -= 1;
                    self.stack.push(current_call);
                }
            }
            _ => {}
        }
    }
}
