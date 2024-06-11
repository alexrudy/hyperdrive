//! Arnold is a [Body](http_body::Body) builder, useful for wrapping
//! different potential bodies for sending responses or requests.
#![cfg_attr(
    feature = "docs",
    doc = r#"
For reading incoming requests, defer to [hyper::body::Incoming].
"#
)]
#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::fmt;
use std::pin::pin;
use std::pin::Pin;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::BodyExt;
use http_body_util::{Empty, Full};

#[cfg(feature = "incoming")]
pub use self::adapt::AdaptIncomingLayer;
#[cfg(feature = "incoming")]
pub use self::adapt::AdaptIncomingService;
pub use self::adapt::{AdaptCustomBodyExt, AdaptCustomBodyLayer, AdaptCustomBodyService};
pub use self::adapt::{AdaptOuterBodyLayer, AdaptOuterBodyService};

type BoxError = Box<dyn std::error::Error + Sync + std::marker::Send + 'static>;

/// An http request using [Body] as the body.
pub type Request = http::Request<Body>;

/// An http response using [Body] as the body.
pub type Response = http::Response<Body>;

/// A wrapper for different internal body types which implements [http_body::Body](http_body::Body)
///
/// Bodies can be created from [`Bytes`](bytes::Bytes), [`String`](std::string::String),
/// or [`&'static str`](str) using [`From`](std::convert::From) implementations.
///
/// An empty body can be created with [Body::empty](Body::empty).
#[derive(Debug)]
#[pin_project::pin_project]
pub struct Body {
    #[pin]
    inner: InnerBody,
}

impl Body {
    /// Create a new `Body` that wraps another [`http_body::Body`].
    pub fn new<B>(body: B) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        try_downcast(body).unwrap_or_else(|body| Self {
            inner: InnerBody::Boxed(Box::pin(body.map_err(Into::into))),
        })
    }

    /// Create a new empty body.
    pub fn empty() -> Self {
        Self {
            inner: InnerBody::Empty,
        }
    }

    /// Convert this body into a boxed body.
    pub fn as_boxed(self) -> UnsyncBoxBody<Bytes, BoxError> {
        match self.inner {
            InnerBody::Boxed(body) => UnsyncBoxBody::new(body),
            InnerBody::Full(body) => UnsyncBoxBody::new(body.map_err(|_| unreachable!())),
            InnerBody::Empty => {
                UnsyncBoxBody::new(http_body_util::Empty::new().map_err(|_| unreachable!()))
            }
            InnerBody::Http(body) => body,
            InnerBody::HttpSync(body) => UnsyncBoxBody::new(body),

            #[cfg(feature = "incoming")]
            InnerBody::Incoming(incoming) => UnsyncBoxBody::new(incoming.map_err(Into::into)),

            #[cfg(feature = "axum")]
            InnerBody::AxumBody(body) => UnsyncBoxBody::new(body.map_err(Into::into)),
        }
    }
}

impl Default for Body {
    fn default() -> Self {
        Self {
            inner: InnerBody::Empty,
        }
    }
}

impl From<Bytes> for Body {
    fn from(body: Bytes) -> Self {
        Self {
            inner: InnerBody::Full(body.into()),
        }
    }
}

impl From<String> for Body {
    fn from(body: String) -> Self {
        Self { inner: body.into() }
    }
}

impl From<&'static str> for Body {
    fn from(body: &'static str) -> Self {
        Self {
            inner: InnerBody::Full(body.into()),
        }
    }
}

impl From<Full<Bytes>> for Body {
    fn from(body: Full<Bytes>) -> Self {
        Self {
            inner: InnerBody::Full(body),
        }
    }
}

impl From<Empty<Bytes>> for Body {
    fn from(_body: Empty<Bytes>) -> Self {
        Self {
            inner: InnerBody::Empty,
        }
    }
}

#[cfg(feature = "incoming")]
impl From<hyper::body::Incoming> for Body {
    fn from(body: hyper::body::Incoming) -> Self {
        Self {
            inner: InnerBody::Incoming(body),
        }
    }
}

#[cfg(feature = "axum")]
impl From<axum::body::Body> for Body {
    fn from(body: axum::body::Body) -> Self {
        Self {
            inner: InnerBody::AxumBody(body),
        }
    }
}

#[cfg(feature = "axum")]
impl From<Body> for axum::body::Body {
    fn from(body: Body) -> Self {
        axum::body::Body::new(body)
    }
}

impl<E> From<UnsyncBoxBody<Bytes, E>> for Body
where
    E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
{
    fn from(body: UnsyncBoxBody<Bytes, E>) -> Self {
        Self {
            inner: InnerBody::Http(
                try_downcast(body)
                    .unwrap_or_else(|body| UnsyncBoxBody::new(body.map_err(Into::into))),
            ),
        }
    }
}

impl From<BoxBody<Bytes, BoxError>> for Body {
    fn from(body: BoxBody<Bytes, BoxError>) -> Self {
        Self {
            inner: InnerBody::HttpSync(body),
        }
    }
}

impl From<Box<dyn http_body::Body<Data = Bytes, Error = BoxError> + Send + 'static>> for Body {
    fn from(
        body: Box<dyn http_body::Body<Data = Bytes, Error = BoxError> + Send + 'static>,
    ) -> Self {
        try_downcast(body).unwrap_or_else(|body| Self {
            inner: InnerBody::Boxed(Box::into_pin(body)),
        })
    }
}

fn try_downcast<T, K>(k: K) -> Result<T, K>
where
    T: 'static,
    K: Send + 'static,
{
    let mut k = Some(k);
    if let Some(k) = <dyn std::any::Any>::downcast_mut::<Option<T>>(&mut k) {
        Ok(k.take().unwrap())
    } else {
        Err(k.unwrap())
    }
}

#[pin_project::pin_project(project = InnerBodyProj)]
enum InnerBody {
    Empty,
    Full(#[pin] Full<Bytes>),
    Boxed(#[pin] Pin<Box<dyn http_body::Body<Data = Bytes, Error = BoxError> + Send + 'static>>),
    Http(#[pin] UnsyncBoxBody<Bytes, BoxError>),
    HttpSync(#[pin] BoxBody<Bytes, BoxError>),

    #[cfg(feature = "incoming")]
    Incoming(#[pin] hyper::body::Incoming),

    #[cfg(feature = "axum")]
    AxumBody(#[pin] axum::body::Body),
}

impl From<String> for InnerBody {
    fn from(body: String) -> Self {
        if body.is_empty() {
            Self::Empty
        } else {
            Self::Full(body.into())
        }
    }
}

macro_rules! poll_frame {
    ($body:ident, $cx:ident) => {
        $body
            .poll_frame($cx)
            .map(|opt| opt.map(|res| res.map_err(Into::into)))
    };
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        match this.inner.project() {
            InnerBodyProj::Empty => std::task::Poll::Ready(None),
            InnerBodyProj::Full(body) => poll_frame!(body, cx),
            InnerBodyProj::Boxed(body) => poll_frame!(body, cx),
            InnerBodyProj::Http(body) => poll_frame!(body, cx),
            InnerBodyProj::HttpSync(body) => poll_frame!(body, cx),
            #[cfg(feature = "incoming")]
            InnerBodyProj::Incoming(body) => poll_frame!(body, cx),

            #[cfg(feature = "axum")]
            InnerBodyProj::AxumBody(body) => poll_frame!(body, cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self.inner {
            InnerBody::Empty => true,
            InnerBody::Full(ref body) => body.is_end_stream(),
            InnerBody::Boxed(ref body) => body.is_end_stream(),
            InnerBody::Http(ref body) => body.is_end_stream(),
            InnerBody::HttpSync(ref body) => body.is_end_stream(),
            #[cfg(feature = "incoming")]
            InnerBody::Incoming(ref body) => body.is_end_stream(),
            #[cfg(feature = "axum")]
            InnerBody::AxumBody(ref body) => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match self.inner {
            InnerBody::Empty => http_body::SizeHint::with_exact(0),
            InnerBody::Full(ref body) => body.size_hint(),
            InnerBody::Boxed(ref body) => body.size_hint(),
            InnerBody::Http(ref body) => body.size_hint(),
            InnerBody::HttpSync(ref body) => body.size_hint(),
            #[cfg(feature = "incoming")]
            InnerBody::Incoming(ref body) => body.size_hint(),
            #[cfg(feature = "axum")]
            InnerBody::AxumBody(ref body) => body.size_hint(),
        }
    }
}

impl fmt::Debug for InnerBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InnerBody::Empty => f.debug_struct("Empty").finish(),
            InnerBody::Full(_) => f.debug_struct("Full").finish(),
            InnerBody::Boxed(_) => f.debug_struct("Boxed").finish(),
            InnerBody::Http(_) => f.debug_struct("Http").finish(),
            InnerBody::HttpSync(_) => f.debug_struct("HttpSync").finish(),
            #[cfg(feature = "incoming")]
            InnerBody::Incoming(_) => f.debug_struct("Incoming").finish(),
            #[cfg(feature = "axum")]
            InnerBody::AxumBody(_) => f.debug_struct("AxumBody").finish(),
        }
    }
}

mod adapt {

    use std::fmt;

    use bytes::Bytes;
    use http_body_util::combinators::UnsyncBoxBody;
    use tower::Layer;
    use tower::Service;

    use super::Body;
    use super::{Request, Response};

    type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

    /// Layer to convert a body to use `Body` as the request body from `hyper::body::Incoming`.
    #[derive(Debug, Clone, Default)]
    pub struct AdaptIncomingLayer;

    impl AdaptIncomingLayer {
        /// Create a new `AdaptBodyLayer`.
        pub fn new() -> Self {
            Self
        }
    }

    impl<S> Layer<S> for AdaptIncomingLayer {
        type Service = AdaptIncomingService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            AdaptIncomingService { inner }
        }
    }

    /// Adapt a service to use `Body` as the request body.
    ///
    /// This is useful when you want to use `Body` as the request body type for a
    /// service, and the outer functions require a service that accepts a body
    /// type of `http::Request<hyper::body::Incoming>`.
    #[derive(Debug, Clone, Default)]
    pub struct AdaptIncomingService<S> {
        inner: S,
    }

    impl<S> AdaptIncomingService<S> {
        /// Create a new `AdaptBody` to wrap a service.
        pub fn new(inner: S) -> Self {
            Self { inner }
        }
    }

    #[cfg(feature = "incoming")]
    impl<T> Service<http::Request<hyper::body::Incoming>> for AdaptIncomingService<T>
    where
        T: Service<Request, Response = Response>,
    {
        type Response = Response;
        type Error = T::Error;
        type Future = T::Future;

        fn poll_ready(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, req: http::Request<hyper::body::Incoming>) -> Self::Future {
            self.inner.call(req.map(Body::from))
        }
    }

    /// Layer to convert a service to use custom body types.
    pub struct AdaptCustomBodyLayer<BIn, BOut> {
        _body: std::marker::PhantomData<fn(BIn) -> BOut>,
    }

    impl<BIn, BOut> fmt::Debug for AdaptCustomBodyLayer<BIn, BOut> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("AdaptCustomBodyLayer").finish()
        }
    }

    impl<BIn, BOut> Clone for AdaptCustomBodyLayer<BIn, BOut> {
        fn clone(&self) -> Self {
            Self::new()
        }
    }

    impl<BIn, BOut> Default for AdaptCustomBodyLayer<BIn, BOut> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<BIn, BOut> AdaptCustomBodyLayer<BIn, BOut> {
        /// Create a new `AdaptCustomBodyLayer`.
        pub fn new() -> Self {
            Self {
                _body: std::marker::PhantomData,
            }
        }
    }

    impl<BIn, BOut, S> Layer<S> for AdaptCustomBodyLayer<BIn, BOut>
    where
        S: Service<http::Request<BOut>>,
    {
        type Service = AdaptCustomBodyService<BIn, BOut, S>;

        fn layer(&self, inner: S) -> Self::Service {
            AdaptCustomBodyService {
                inner,
                _body: std::marker::PhantomData,
            }
        }
    }

    /// Adapt a service to use custom body types internally,
    /// while still accepting and returning `Body` as the outer body type.
    ///
    /// This is useful when interfacing with other libraries which want to bring their
    /// own body types, but you want to use `Body` as the outer body type and use `hyperdriver::Server`.
    pub struct AdaptCustomBodyService<BIn, BOut, S> {
        inner: S,
        _body: std::marker::PhantomData<fn(BIn) -> BOut>,
    }

    impl<BIn, BOut, S: fmt::Debug> fmt::Debug for AdaptCustomBodyService<BIn, BOut, S> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("AdaptCustomBodyService")
                .field("inner", &self.inner)
                .finish()
        }
    }

    impl<BIn, BOut, S: Clone> Clone for AdaptCustomBodyService<BIn, BOut, S> {
        fn clone(&self) -> Self {
            Self::new(self.inner.clone())
        }
    }

    impl<BIn, BOut, S: Default> Default for AdaptCustomBodyService<BIn, BOut, S> {
        fn default() -> Self {
            Self::new(S::default())
        }
    }

    impl<BIn, BOut, S> AdaptCustomBodyService<BIn, BOut, S> {
        /// Create a new `AdaptCustomBodyService`.
        pub fn new(inner: S) -> Self {
            Self {
                inner,
                _body: std::marker::PhantomData,
            }
        }
    }

    impl<BIn, BOut, S> Service<http::Request<super::Body>> for AdaptCustomBodyService<BIn, BOut, S>
    where
        S: Service<http::Request<BIn>, Response = http::Response<BOut>>,
        BIn: From<UnsyncBoxBody<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>,
        BOut: http_body::Body<Data = Bytes> + Send + 'static,
        BOut::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        type Response = http::Response<super::Body>;
        type Error = S::Error;
        type Future = fut::AdaptBodyFuture<S::Future, BOut, S::Error>;

        fn poll_ready(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, req: http::Request<super::Body>) -> Self::Future {
            fut::AdaptBodyFuture::new(self.inner.call(req.map(|b| b.as_boxed().into())))
        }
    }

    mod fut {
        use super::BoxError;
        use bytes::Bytes;
        use pin_project::pin_project;
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        type PhantomServiceFuture<Body, Error> =
            std::marker::PhantomData<fn() -> Result<http::Response<Body>, Error>>;

        #[pin_project]
        #[derive(Debug)]
        pub struct AdaptBodyFuture<Fut, Body, Error> {
            #[pin]
            inner: Fut,
            _body: PhantomServiceFuture<Body, Error>,
        }

        impl<Fut, Body, Error> AdaptBodyFuture<Fut, Body, Error> {
            pub(super) fn new(inner: Fut) -> Self {
                Self {
                    inner,
                    _body: std::marker::PhantomData,
                }
            }
        }

        impl<Fut, Body, Error> Future for AdaptBodyFuture<Fut, Body, Error>
        where
            Fut: Future<Output = Result<http::Response<Body>, Error>>,
            Body: http_body::Body<Data = Bytes> + Send + 'static,
            Body::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        {
            type Output = Result<http::Response<crate::body::Body>, Error>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();
                this.inner
                    .poll(cx)
                    .map(|res| res.map(|res| res.map(crate::Body::new)))
            }
        }

        #[derive(Debug)]
        #[pin_project]
        pub struct AdaptOuterBodyFuture<Fut, Body, Error> {
            #[pin]
            inner: Fut,
            _body: PhantomServiceFuture<Body, Error>,
        }

        impl<Fut, BodyOut, Error> AdaptOuterBodyFuture<Fut, BodyOut, Error> {
            pub(super) fn new(inner: Fut) -> Self {
                Self {
                    inner,
                    _body: std::marker::PhantomData,
                }
            }
        }

        impl<Fut, BodyOut, Error> Future for AdaptOuterBodyFuture<Fut, BodyOut, Error>
        where
            Fut: Future<Output = Result<http::Response<crate::body::Body>, Error>>,
            BodyOut: From<http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>>,
        {
            type Output = Result<http::Response<BodyOut>, Error>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();
                match this.inner.poll(cx) {
                    Poll::Ready(Ok(res)) => Poll::Ready(Ok(res.map(|body| body.as_boxed().into()))),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    /// Extension trait for `Service` to adapt inner body types to crate::Body.
    pub trait AdaptCustomBodyExt<BIn, BOut>: Sized {
        /// Adapt a service to use custom body types internally, but still accept and return
        /// `Body` as the outer body type.
        fn adapt_custom_body(self) -> AdaptCustomBodyService<BIn, BOut, Self>;
    }

    impl<BIn, BOut, S> AdaptCustomBodyExt<BIn, BOut> for S
    where
        S: Service<http::Request<BIn>, Response = http::Response<BOut>>,
    {
        fn adapt_custom_body(self) -> AdaptCustomBodyService<BIn, BOut, S> {
            AdaptCustomBodyService::new(self)
        }
    }

    /// Adapt a service to externally accept and return a custom body type, but internally use
    /// `crate::Body`.
    pub struct AdaptOuterBodyLayer<BIn, BOut> {
        _body: std::marker::PhantomData<fn(BIn) -> BOut>,
    }

    impl<BIn, BOut> Default for AdaptOuterBodyLayer<BIn, BOut> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<BIn, BOut> Clone for AdaptOuterBodyLayer<BIn, BOut> {
        fn clone(&self) -> Self {
            Self::new()
        }
    }

    impl<BIn, BOut> fmt::Debug for AdaptOuterBodyLayer<BIn, BOut> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("AdaptOuterBodyLayer").finish()
        }
    }

    impl<BIn, BOut> AdaptOuterBodyLayer<BIn, BOut> {
        /// Create a new `AdaptOuterBodyLayer`.
        pub fn new() -> Self {
            Self {
                _body: std::marker::PhantomData,
            }
        }
    }

    impl<BIn, BOut, S> Layer<S> for AdaptOuterBodyLayer<BIn, BOut>
    where
        S: Service<http::Request<BIn>>,
    {
        type Service = AdaptOuterBodyService<BIn, BOut, S>;

        fn layer(&self, inner: S) -> Self::Service {
            AdaptOuterBodyService {
                inner,
                _body: std::marker::PhantomData,
            }
        }
    }

    /// Service to accept and return a custom body type, but internally use `crate::Body`.
    pub struct AdaptOuterBodyService<BIn, BOut, S> {
        inner: S,
        _body: std::marker::PhantomData<fn(BIn) -> BOut>,
    }

    impl<BIn, BOut, S: fmt::Debug> fmt::Debug for AdaptOuterBodyService<BIn, BOut, S> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("AdaptOuterBodyService")
                .field("inner", &self.inner)
                .finish()
        }
    }

    impl<BIn, BOut, S: Clone> Clone for AdaptOuterBodyService<BIn, BOut, S> {
        fn clone(&self) -> Self {
            Self::new(self.inner.clone())
        }
    }

    impl<BIn, BOut, S: Default> Default for AdaptOuterBodyService<BIn, BOut, S> {
        fn default() -> Self {
            Self::new(S::default())
        }
    }

    impl<BIn, BOut, S> AdaptOuterBodyService<BIn, BOut, S> {
        /// Create a new `AdaptOuterBodyService`.
        pub fn new(inner: S) -> Self {
            Self {
                inner,
                _body: std::marker::PhantomData,
            }
        }
    }

    impl<BIn, BOut, S> Service<http::Request<BIn>> for AdaptOuterBodyService<BIn, BOut, S>
    where
        S: Service<http::Request<super::Body>, Response = http::Response<super::Body>>,
        BIn: Into<super::Body>,
        BOut: From<http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>>,
    {
        type Response = http::Response<BOut>;
        type Error = S::Error;
        type Future = fut::AdaptOuterBodyFuture<S::Future, BOut, S::Error>;

        fn poll_ready(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, req: http::Request<BIn>) -> Self::Future {
            fut::AdaptOuterBodyFuture::new(self.inner.call(req.map(Into::into)))
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use static_assertions::assert_impl_all;

    assert_impl_all!(Body: Send);
}
