# ProjectBinaryTarget

Target OS and architecture for compiled binaries.

Used as keys in package output maps (CLI binaries, Terraform providers, etc.)
and for cross-compilation target selection during builds.

## Example Usage

```typescript
import { ProjectBinaryTarget } from "@alienplatform/platform-api/models";

let value: ProjectBinaryTarget = "linux-x64";
```

## Values

```typescript
"windows-x64" | "linux-x64" | "linux-arm64" | "darwin-arm64"
```