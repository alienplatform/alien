//! Steps a controller library's controllers in a native test, with the test in place of Alien.

use super::cloud::host::ANSWER;
use super::Controllers;
use serde_json::Value;

/// Answers `request` (a step of the step protocol, or its `load` or `controllers` op, as the
/// JSON Alien sends) as the module built from `register` answers it, and returns the reply.
///
/// `host` answers each call a handler makes, through [`Cloud`](super::Cloud) or an AWS SDK
/// client: it gets the request as the host would (`service`, `method`, `path`, `query`,
/// `headers` and `body`, or `url` for a declared origin) and returns the response
/// (`{ "status": 200, "headers": [], "body": "..." }`), or the line a host that sent no
/// response answers with: `refused: ...` or `failed: ...`.
///
/// `Err` is a request the module can't answer in the protocol, such as one made while two
/// controllers are registered for one type and platform.
pub fn answer(
    register: fn(&mut Controllers),
    request: &Value,
    host: impl FnMut(&Value) -> Result<Value, String> + 'static,
) -> Result<Value, String> {
    /// Takes the test's host away again, also when a handler panics.
    struct Installed;
    impl Drop for Installed {
        fn drop(&mut self) {
            ANSWER.with(|answer| answer.borrow_mut().take());
        }
    }
    ANSWER.with(|answer| answer.borrow_mut().replace(Box::new(host)));
    let _installed = Installed;
    let mut controllers = Controllers::default();
    register(&mut controllers);
    controllers.answer(request.to_string().as_bytes())
}
