# SyncRenewRequest

## Example Usage

```typescript
import { SyncRenewRequest } from "@alienplatform/platform-api/models";

let value: SyncRenewRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  deploymentIds: [
    "dep_0c29fq4a2yjb7kx3smwdgxlc",
  ],
  session: "<value>",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    | Example                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                                 | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Single deployment to renew. Deprecated: use deploymentIds. Retained so a manager built against the previous API keeps renewing during rollout. | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                                                   |
| `deploymentIds`                                                                                                                                | *string*[]                                                                                                                                     | :heavy_minus_sign:                                                                                                                             | Deployments to renew or confirm. Renewal is per deployment: the response reports each one independently.                                       |                                                                                                                                                |
| `session`                                                                                                                                      | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |                                                                                                                                                |