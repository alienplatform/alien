# Claim

## Example Usage

```typescript
import { Claim } from "@alienplatform/platform-api/models";

let value: Claim = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  operationId: "duop_0vtxpb1sw4sbcdwg2xo37q6",
  attemptId: "duat_uve04tou5eoua3q17dar1pz",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            | Example                                                |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `deploymentId`                                         | *string*                                               | :heavy_check_mark:                                     | Unique identifier for the deployment.                  | dep_0c29fq4a2yjb7kx3smwdgxlc                           |
| `operationId`                                          | *string*                                               | :heavy_check_mark:                                     | Unique identifier for the deployment update operation. | duop_0vtxpb1sw4sbcdwg2xo37q6                           |
| `attemptId`                                            | *string*                                               | :heavy_check_mark:                                     | Unique identifier for the deployment update attempt.   | duat_uve04tou5eoua3q17dar1pz                           |