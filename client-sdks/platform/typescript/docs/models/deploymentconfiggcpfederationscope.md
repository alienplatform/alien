# DeploymentConfigGcpFederationScope

Identity of one installation. The runtime constructs this from its
authenticated deployment record, never names or imported target metadata.

## Example Usage

```typescript
import { DeploymentConfigGcpFederationScope } from "@alienplatform/platform-api/models";

let value: DeploymentConfigGcpFederationScope = {
  deploymentId: "<id>",
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                               | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | Immutable ID allocated by the control plane before cloud setup.<br/>Names, display metadata and imported resource identifiers cannot replace it. |