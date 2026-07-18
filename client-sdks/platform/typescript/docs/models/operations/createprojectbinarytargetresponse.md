# CreateProjectBinaryTargetResponse

Target OS and architecture for compiled binaries.

Used as keys in package output maps (CLI binaries, Terraform providers, etc.)
and for cross-compilation target selection during builds.

## Example Usage

```typescript
import { CreateProjectBinaryTargetResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectBinaryTargetResponse = "linux-x64";
```

## Values

```typescript
"windows-x64" | "linux-x64" | "linux-arm64" | "darwin-arm64"
```