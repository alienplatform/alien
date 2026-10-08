# Python container

This example builds a locked Python project without an application Dockerfile.
It imports the Alien Python SDK and serves a small HTTP readiness response.

Until `alienplatform` is published, point the build at a wheel for the target
architecture:

```sh
ALIEN_PYTHON_SDK_WHEEL=/path/to/alienplatform.whl alien build --platforms local
```
