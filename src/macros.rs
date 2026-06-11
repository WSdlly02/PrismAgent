//! Declarative macros for reducing actor boilerplate.
//!
//! The PrismAgent actor model follows a consistent pattern for each actor:
//! - A **Message enum** (`XxxMsg`) with variants containing
//!   `reply: oneshot::Sender<SubsystemResult<T>>`
//! - An **Actor struct** (`XxxActor`) owning the `mpsc::Receiver<XxxMsg>`
//! - A **Handle struct** (`XxxHandle`) owning the `mpsc::Sender<XxxMsg>`
//!   with async convenience methods
//! - A **run() method** that dispatches messages to actor methods via `match`
//!
//! This module provides three macros to eliminate the repetitive boilerplate.
//!
//! # Quick Reference
//!
//! | Macro | Generates |
//! |-------|-----------|
//! | [`actor_dispatch!`] | `match` arms for `run()` message dispatch |
//! | [`impl_handle_methods!`] | Handle convenience methods |
//! | [`impl_actor!`] | Combined: `run()` dispatch + Handle methods |
//!
//! # Usage Example — Before
//!
//! ```ignore
//! // Manual Handle methods (repetitive ~8 lines each)
//! impl WorkspaceHandle {
//!     pub async fn list(&self) -> SubsystemResult<Vec<WorkspaceSummary>> {
//!         let (reply_tx, reply_rx) = oneshot::channel();
//!         self.tx.send(WorkspaceMsg::List { reply: reply_tx }).await
//!             .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
//!         reply_rx.await
//!             .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
//!     }
//!     pub async fn create(&self, r: WorkspaceCreateRequest)
//!         -> SubsystemResult<WorkspaceSummary>
//!     {
//!         let (reply_tx, reply_rx) = oneshot::channel();
//!         self.tx.send(WorkspaceMsg::Create { request: r, reply: reply_tx }).await
//!             .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
//!         reply_rx.await
//!             .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
//!     }
//! }
//!
//! // Manual run() dispatch
//! impl WorkspaceActor {
//!     pub async fn run(mut self) {
//!         while let Some(msg) = self.rx.recv().await {
//!             match msg {
//!                 WorkspaceMsg::List { reply } => {
//!                     let _ = reply.send(self.list());
//!                 }
//!                 WorkspaceMsg::Create { request, reply } => {
//!                     let _ = reply.send(self.create(request).await);
//!                 }
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! # Usage Example — After
//!
//! ```ignore
//! use crate::actor_dispatch;
//! use crate::impl_handle_methods;
//!
//! // One macro call replaces all Handle methods
//! impl_handle_methods! {
//!     WorkspaceHandle for WorkspaceMsg, WORKSPACE_ACTOR;
//!
//!     fn list(&self) -> Vec<WorkspaceSummary>
//!         => List {};
//!
//!     fn create(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
//!         => Create { request: request };
//! }
//!
//! // One macro call replaces the entire run() dispatch
//! impl WorkspaceActor {
//!     pub async fn run(mut self) {
//!         while let Some(msg) = self.rx.recv().await {
//!             actor_dispatch!(msg;
//!                 WorkspaceMsg::List { ; reply } => self.list(),
//!                 WorkspaceMsg::Create { request ; reply } => self.create(request).await,
//!             );
//!         }
//!     }
//! }
//!
//! // Or use the combined macro for both at once:
//! impl_actor! {
//!     actor WorkspaceActor;
//!     handle WorkspaceHandle;
//!     msg WorkspaceMsg;
//!     actor_name WORKSPACE_ACTOR;
//!     methods {
//!         fn list(&self) -> Vec<WorkspaceSummary>
//!             => List {};
//!         fn create(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
//!             => Create { request: request };
//!     }
//! }
//! ```

use crate::error::{SubsystemError, SubsystemResult};
use tokio::sync::{mpsc, oneshot};

// ============================================================================
// Helper function
// ============================================================================

/// Internal helper used by [`impl_handle_methods!`].
///
/// Creates a oneshot channel, wraps it in a message via the provided closure,
/// sends the message through the actor's mpsc channel, and awaits the reply.
///
/// **Not intended for direct use** — use the macros instead.
pub async fn _request<T, M>(
    tx: &mpsc::Sender<M>,
    build_msg: impl FnOnce(oneshot::Sender<SubsystemResult<T>>) -> M,
    actor_name: &'static str,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(build_msg(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(actor_name))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(actor_name))?
}

// ============================================================================
// actor_dispatch!
// ============================================================================

/// Generates match arms for actor message dispatch in `run()`.
///
/// Each arm destructures a message variant (binding `reply` and other fields),
/// executes a handler expression, and sends the result through `reply`.
///
/// # Syntax
///
/// Fields before `;` are the non-reply match bindings; the identifier after
/// `;` is the reply channel.
///
/// ```text
/// actor_dispatch!(msg_expr;
///     Msg::Variant { field1, field2 ; reply } => handler,
/// );
/// ```
///
/// # Example
///
/// ```ignore
/// impl Actor {
///     pub async fn run(mut self) {
///         while let Some(msg) = self.rx.recv().await {
///             actor_dispatch!(msg;
///                 Msg::List { ; reply } => self.list(),
///                 Msg::Create { request ; reply } => self.create(request).await,
///             );
///         }
///     }
/// }
/// ```
///
/// # Notes
///
/// - `reply` must be the **last** identifier, separated from other fields by
///   `;`. The macro captures it as a metavariable to ensure correct hygiene.
/// - Handler expressions must return `SubsystemResult<T>` matching the reply
///   channel type.
/// - For fire-and-forget variants (no `reply`), write a regular `match` arm
///   outside the macro call.
#[macro_export]
macro_rules! actor_dispatch {
    ($msg:expr;
        $( $Msg:ident::$Variant:ident { $($field:ident),* $(,)? ; $reply:ident } => $handler:expr ),*
        $(,)?
    ) => {
        match $msg {
            $(
                $Msg::$Variant { $($field,)* $reply } => {
                    let _ = $reply.send($handler);
                }
            )*
        }
    };
}

// ============================================================================
// impl_handle_methods!
// ============================================================================

/// Generates Handle convenience methods for an actor.
///
/// For each method specification, the macro generates a `pub async fn` on the
/// Handle type that:
/// 1. Creates a oneshot reply channel
/// 2. Constructs the message variant with the given field expressions and `reply`
/// 3. Sends the message through the actor's mpsc channel
/// 4. Awaits and returns the reply
///
/// # Syntax
///
/// ```text
/// impl_handle_methods! {
///     HandleType for MsgType, ACTOR_NAME;
///
///     fn method_name(&self, param1: Type1) -> ReturnType
///         => VariantName { field1: expr1, field2: expr2 };
///
///     // additional methods...
/// }
/// ```
///
/// The `reply` field is **automatically appended** to each variant — do not
/// include it in the field list.
///
/// # Example
///
/// ```ignore
/// impl_handle_methods! {
///     WorkspaceHandle for WorkspaceMsg, WORKSPACE_ACTOR;
///
///     fn list(&self) -> Vec<WorkspaceSummary>
///         => List {};
///
///     fn create(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
///         => Create { request: request };
///
///     fn contains(&self, workspace_uuid: impl Into<String>) -> bool
///         => Contains { workspace_uuid: workspace_uuid.into() };
/// }
/// ```
#[macro_export]
macro_rules! impl_handle_methods {
    (
        $Handle:ident for $Msg:ident, $ACTOR:expr;
        $(
            fn $method:ident(&self $(, $param:ident : $ptype:ty)*) -> $ret:ty
                => $Variant:ident { $($fname:ident : $fval:expr),* $(,)? }
        );* $(;)?
    ) => {
        impl $Handle {
            $(
                pub async fn $method(
                    &self
                    $(, $param: $ptype)*
                ) -> $crate::error::SubsystemResult<$ret> {
                    $crate::macros::_request(&self.tx, |reply| {
                        $Msg::$Variant { $($fname: $fval,)* reply }
                    }, $ACTOR).await
                }
            )*
        }
    };
}

// ============================================================================
// impl_actor!
// ============================================================================

/// Combined macro: generates both `run()` message dispatch and Handle methods.
///
/// Produces:
/// 1. `impl Actor { pub async fn run(mut self) { ... } }` — with
///    [`actor_dispatch!`] for each specified method
/// 2. `impl Handle { ... }` — with convenience methods via
///    [`impl_handle_methods!`]
///
/// # Requirements
///
/// - The Actor struct must have a `rx: mpsc::Receiver<Msg>` field.
/// - All dispatch methods must be `async fn` (even if they don't use `.await`
///   internally — wrap synchronous bodies with `async fn`).
/// - Method parameter order must match the message variant field order.
///
/// # Example
///
/// ```ignore
/// impl_actor! {
///     actor WorkspaceActor;
///     handle WorkspaceHandle;
///     msg WorkspaceMsg;
///     actor_name WORKSPACE_ACTOR;
///     methods {
///         fn list(&self) -> Vec<WorkspaceSummary>
///             => List {};
///
///         fn create(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
///             => Create { request: request };
///     }
/// }
/// ```
///
/// # Limitations
///
/// - Fire-and-forget variants (no `reply` channel) cannot be expressed with
///   this macro. Use [`actor_dispatch!`] directly in a manually-written `run()`
///   for those variants.
/// - If any dispatch method must be synchronous (`fn` instead of `async fn`),
///   either make it `async fn` or use [`actor_dispatch!`] directly.
#[macro_export]
macro_rules! impl_actor {
    (
        actor $Actor:ident;
        handle $Handle:ident;
        msg $Msg:ident;
        actor_name $ACTOR:expr;
        methods {
            $(
                fn $method:ident(&self $(, $param:ident : $ptype:ty)*) -> $ret:ty
                    => $Variant:ident { $($fname:ident : $fval:expr),* $(,)? }
            );* $(;)?
        }
    ) => {
        impl $Actor {
            /// Runs the actor message loop, dispatching each incoming message
            /// to the appropriate handler method.
            pub async fn run(mut self) {
                while let Some(msg) = self.rx.recv().await {
                    $crate::actor_dispatch!(msg;
                        $(
                            $Msg::$Variant { $($fname,)* ; reply } =>
                                self.$method($($fname),*).await
                        ),*
                    );
                }
            }
        }

        $crate::impl_handle_methods! {
            $Handle for $Msg, $ACTOR;
            $(
                fn $method(&self $(, $param : $ptype)*) -> $ret
                    => $Variant { $($fname : $fval),* }
            );*
        }
    };
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::error::SubsystemResult;
    use tokio::sync::{mpsc, oneshot};

    // -----------------------------------------------------------------------
    // Test 1: actor_dispatch! + impl_handle_methods! used separately
    // -----------------------------------------------------------------------

    const TEST_ACTOR: &str = "test";

    #[derive(Clone)]
    struct TestHandle {
        tx: mpsc::Sender<TestMsg>,
    }

    struct TestActor {
        rx: mpsc::Receiver<TestMsg>,
    }

    enum TestMsg {
        Echo {
            text: String,
            reply: oneshot::Sender<SubsystemResult<String>>,
        },
        Add {
            a: i32,
            b: i32,
            reply: oneshot::Sender<SubsystemResult<i32>>,
        },
    }

    impl TestActor {
        fn load(rx: mpsc::Receiver<TestMsg>) -> Self {
            Self { rx }
        }

        async fn echo(&self, text: String) -> SubsystemResult<String> {
            Ok(text)
        }

        async fn add(&self, a: i32, b: i32) -> SubsystemResult<i32> {
            Ok(a + b)
        }

        pub async fn run(mut self) {
            while let Some(msg) = self.rx.recv().await {
                actor_dispatch!(msg;
                    TestMsg::Echo { text ; reply } => self.echo(text).await,
                    TestMsg::Add { a, b ; reply } => self.add(a, b).await,
                );
            }
        }
    }

    impl_handle_methods! {
        TestHandle for TestMsg, TEST_ACTOR;

        fn echo(&self, text: String) -> String
            => Echo { text: text };

        fn add(&self, a: i32, b: i32) -> i32
            => Add { a: a, b: b };
    }

    #[tokio::test]
    async fn actor_dispatch_echo() {
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(TestActor::load(rx).run());
        let handle = TestHandle { tx };

        assert_eq!(handle.echo("hello".into()).await.unwrap(), "hello");
    }

    #[tokio::test]
    async fn actor_dispatch_add() {
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(TestActor::load(rx).run());
        let handle = TestHandle { tx };

        assert_eq!(handle.add(2, 3).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn handle_concurrent_requests() {
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(TestActor::load(rx).run());
        let handle = TestHandle { tx };

        let (echo, add) = tokio::join!(handle.echo("parallel".into()), handle.add(10, 20));
        assert_eq!(echo.unwrap(), "parallel");
        assert_eq!(add.unwrap(), 30);
    }

    #[tokio::test]
    async fn handle_returns_error_when_actor_dead() {
        let (tx, rx) = mpsc::channel(8);
        drop(rx); // actor never started — channel closed
        let handle = TestHandle { tx };

        let result = handle.echo("dead".into()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::SubsystemError::ActorDead { actor } => {
                assert_eq!(actor, TEST_ACTOR);
            }
            other => panic!("expected ActorDead, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Test 2: impl_actor! combined macro
    // -----------------------------------------------------------------------

    mod combined {
        use super::*;

        const COMBINED_ACTOR: &str = "combined";

        #[derive(Clone)]
        struct CombinedHandle {
            tx: mpsc::Sender<CombinedMsg>,
        }

        struct CombinedActor {
            rx: mpsc::Receiver<CombinedMsg>,
            prefix: String,
        }

        enum CombinedMsg {
            Greet {
                name: String,
                reply: oneshot::Sender<SubsystemResult<String>>,
            },
            Multiply {
                a: i32,
                b: i32,
                reply: oneshot::Sender<SubsystemResult<i32>>,
            },
        }

        impl CombinedActor {
            async fn greet(&mut self, name: String) -> SubsystemResult<String> {
                Ok(format!("{}, {}!", self.prefix, name))
            }

            async fn multiply(&self, a: i32, b: i32) -> SubsystemResult<i32> {
                Ok(a * b)
            }
        }

        impl_actor! {
            actor CombinedActor;
            handle CombinedHandle;
            msg CombinedMsg;
            actor_name COMBINED_ACTOR;
            methods {
                fn greet(&self, name: String) -> String
                    => Greet { name: name };

                fn multiply(&self, a: i32, b: i32) -> i32
                    => Multiply { a: a, b: b };
            }
        }

        #[tokio::test]
        async fn combined_greet() {
            let (tx, rx) = mpsc::channel(8);
            let actor = CombinedActor {
                rx,
                prefix: "Hi".to_string(),
            };
            tokio::spawn(actor.run());
            let handle = CombinedHandle { tx };

            assert_eq!(handle.greet("World".into()).await.unwrap(), "Hi, World!");
        }

        #[tokio::test]
        async fn combined_multiply() {
            let (tx, rx) = mpsc::channel(8);
            let actor = CombinedActor {
                rx,
                prefix: String::new(),
            };
            tokio::spawn(actor.run());
            let handle = CombinedHandle { tx };

            assert_eq!(handle.multiply(3, 7).await.unwrap(), 21);
        }

        #[tokio::test]
        async fn combined_actor_dead_error() {
            let (tx, rx) = mpsc::channel(8);
            drop(rx); // actor never started
            let handle = CombinedHandle { tx };

            let result = handle.greet("test".into()).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                crate::error::SubsystemError::ActorDead { actor } => {
                    assert_eq!(actor, COMBINED_ACTOR);
                }
                other => panic!("expected ActorDead, got {other:?}"),
            }
        }
    }
}
