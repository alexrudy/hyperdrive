use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project::pin_project;

pub(crate) fn lazy<F, R>(f: F) -> Lazy<F, R>
where
    F: FnOnce() -> R,
    R: Future,
{
    Lazy {
        inner: InnerLazy::Init(f),
    }
}

#[pin_project]
pub(crate) struct Lazy<F, R> {
    #[pin]
    inner: InnerLazy<F, R>,
}

#[pin_project(project = InnerProj, project_replace = InnerProjReplace)]
enum InnerLazy<F, R> {
    Init(F),
    Future(#[pin] R),
    Empty,
}

impl<F, R> Future for Lazy<F, R>
where
    F: FnOnce() -> R,
    R: Future,
{
    type Output = R::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if let InnerProj::Future(future) = this.inner.as_mut().project() {
            return future.poll(cx);
        }

        if let InnerProjReplace::Init(f) = this.inner.as_mut().project_replace(InnerLazy::Empty) {
            this.inner.set(InnerLazy::Future(f()));
        }

        if let InnerProj::Future(future) = this.inner.as_mut().project() {
            return future.poll(cx);
        }

        unreachable!("lazy future is empty");
    }
}
