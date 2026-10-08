# ExternalBindingLocal

Local container registry binding configuration.

The local registry runs on localhost only and does not require authentication.
Security boundary is the OS process isolation on the customer's machine.
External image access is secured by the manager's registry proxy (deployment tokens).

## Example Usage

```typescript
import { ExternalBindingLocal } from "@alienplatform/platform-api/models";

let value: ExternalBindingLocal = {
  dataDir: "<value>",
  registryUrl: "https://fixed-alligator.org",
  service: "local",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `registryUrl`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeArtifactRegistry4](../models/typeartifactregistry4.md)                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
