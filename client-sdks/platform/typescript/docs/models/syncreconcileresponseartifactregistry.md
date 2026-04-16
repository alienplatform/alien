# SyncReconcileResponseArtifactRegistry

Artifact registry configuration for pulling container images.

Used when the deployment needs to pull images from a manager's artifact registry.
This is required for Local platform and can optionally be used by cloud platforms
instead of native registry mechanisms (ECR/GCR/ACR).

## Example Usage

```typescript
import { SyncReconcileResponseArtifactRegistry } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseArtifactRegistry = {
  managerUrl: "https://misguided-impostor.net",
};
```

## Fields

| Field                                                                                                                                 | Type                                                                                                                                  | Required                                                                                                                              | Description                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `authToken`                                                                                                                           | *string*                                                                                                                              | :heavy_minus_sign:                                                                                                                    | Optional authentication token (JWT) for manager API access<br/>When present, must be included in Authorization header as "Bearer {token}" |
| `managerUrl`                                                                                                                          | *string*                                                                                                                              | :heavy_check_mark:                                                                                                                    | Manager base URL for fetching credentials and accessing the registry                                                                  |