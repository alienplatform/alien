# ManagerSetupUpdateRequest

## Example Usage

```typescript
import { ManagerSetupUpdateRequest } from "@alienplatform/platform-api/models";

let value: ManagerSetupUpdateRequest = {};
```

## Fields

| Field                                                                                                                                             | Type                                                                                                                                              | Required                                                                                                                                          | Description                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentRegions`                                                                                                                               | *string*[]                                                                                                                                        | :heavy_minus_sign:                                                                                                                                | AWS regions workloads of the instance's deployments run in. The setup keeps any image replication rules the account already has and adds Alien's. |