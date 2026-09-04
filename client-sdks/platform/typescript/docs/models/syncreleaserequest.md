# SyncReleaseRequest

Request to release deployment lock

## Example Usage

```typescript
import { SyncReleaseRequest } from "@alienplatform/platform-api/models";

let value: SyncReleaseRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  session: "<value>",
  operationId: "duop_0vtxpb1sw4sbcdwg2xo37q6",
  attemptId: "duat_uve04tou5eoua3q17dar1pz",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 | Example                                                                     |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `deploymentId`                                                              | *string*                                                                    | :heavy_check_mark:                                                          | Deployment ID to release                                                    | dep_0c29fq4a2yjb7kx3smwdgxlc                                                |
| `session`                                                                   | *string*                                                                    | :heavy_check_mark:                                                          | Session identifier to release                                               |                                                                             |
| `operationId`                                                               | *string*                                                                    | :heavy_minus_sign:                                                          | Operation returned by acquire. Required when the lease carried update work. | duop_0vtxpb1sw4sbcdwg2xo37q6                                                |
| `attemptId`                                                                 | *string*                                                                    | :heavy_minus_sign:                                                          | Attempt returned by acquire. Required when the lease carried update work.   | duat_uve04tou5eoua3q17dar1pz                                                |