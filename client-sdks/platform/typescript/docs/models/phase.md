# Phase

Phase of a deployment at which a failure occurred.

Derived from the source deployment status: `preflights-failed` →
`Preflights`, `provisioning-failed` → `Provisioning`, `update-failed` →
`Updating`, `delete-failed` → `Deleting`.
`refresh-failed` is modelled separately via `DeploymentDegraded`.

## Example Usage

```typescript
import { Phase } from "@alienplatform/platform-api/models";

let value: Phase = "provisioning";
```

## Values

```typescript
"preflights" | "provisioning" | "updating" | "deleting"
```