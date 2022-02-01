use std::sync::mpsc::RecvError;
use std::time::Duration;
use cogo::{go, select};
use cogo::coroutine::sleep;
use cogo::std::context::{CancelCtx, Canceler};
use cogo::std::errors::Error;

//TODO Please note that,This example is not stable yet
fn main() {
    let mut ctx = CancelCtx::new_arc(None);

    ctx.cancel(Some(Error::from("EOF")));

    loop {
        let mut break_self = false;
        select! {
                v = ctx.done().unwrap().recv() =>{
                    println!("done");
                   break_self = true;
                }
        }
        ;
        if break_self {
            break;
        }
    }
}