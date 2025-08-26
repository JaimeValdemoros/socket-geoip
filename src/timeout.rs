pub trait FutureExt: Future {
    async fn timeout(self, dur: std::time::Duration) -> Option<Self::Output>
    where
        Self: Sized,
    {
        smol::future::or(async { Some(self.await) }, async {
            smol::Timer::after(dur).await;
            None
        })
        .await
    }
}

impl<F: Future> FutureExt for F {}
