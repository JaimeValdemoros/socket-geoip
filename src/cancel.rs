use std::sync::{Arc, Mutex};

use futures::{FutureExt as _, future::Shared};

#[derive(Clone)]
pub struct Token {
    rx: Shared<oneshot::Receiver<()>>,
}

impl Token {
    pub fn new() -> (Self, impl Fn() -> () + Clone) {
        let (tx, rx) = oneshot::channel::<()>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        (Token { rx: rx.shared() }, move || {
            let _ = tx.lock().unwrap().take().map(|tx| tx.send(()));
        })
    }
}

pub trait FutureExt: Future {
    fn with_cancel(self, token: &Token) -> impl Future<Output = Option<Self::Output>>
    where
        Self: Sized,
    {
        let mut rx = token.rx.clone();
        let fut = self.fuse();
        async move {
            futures::pin_mut!(fut);
            futures::select_biased! {
                _ = rx => None,
                x = fut => Some(x),
            }
        }
    }
}

impl<F: Future> FutureExt for F {}
