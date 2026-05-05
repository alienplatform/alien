# PersistImportedDeploymentRequestExternalBindings

External bindings for pre-existing infrastructure.
Allows using existing resources (MinIO, Redis, shared Container Apps
Environment, etc.) instead of having Alien provision them.
Required for Kubernetes platform, optional for cloud platforms.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExternalBindings } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExternalBindings = {};
```

## Fields

| Field       | Type        | Required    | Description |
| ----------- | ----------- | ----------- | ----------- |