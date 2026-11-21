# SyncRenewRequest

## Example Usage

```typescript
import { SyncRenewRequest } from "@alienplatform/platform-api/models";

let value: SyncRenewRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  deploymentIds: [
    "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  ],
  claims: [
    {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
      operationId: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
      attemptId: "duat_uve04tou5eoua3q17dar1pzsbnjr",
    },
  ],
  session: "<value>",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    | Example                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                                 | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Single deployment to renew. Deprecated: use deploymentIds. Retained so a manager built against the previous API keeps renewing during rollout. | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                                                                               |
| `deploymentIds`                                                                                                                                | *string*[]                                                                                                                                     | :heavy_minus_sign:                                                                                                                             | Deployments to renew or confirm. Renewal is per deployment: the response reports each one independently.                                       |                                                                                                                                                |
| `claims`                                                                                                                                       | [models.Claim](../models/claim.md)[]                                                                                                           | :heavy_minus_sign:                                                                                                                             | Claim identities for deployments currently executing an update.                                                                                |                                                                                                                                                |
| `session`                                                                                                                                      | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |                                                                                                                                                |