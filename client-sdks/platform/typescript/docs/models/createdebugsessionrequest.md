# CreateDebugSessionRequest

## Example Usage

```typescript
import { CreateDebugSessionRequest } from "@alienplatform/platform-api/models";

let value: CreateDebugSessionRequest = {
  id: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  owner: "<value>",
  expiresAt: new Date("2024-08-15T01:20:10.131Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_minus_sign:                                                                            | Override the generated id. Manager passes the registry session id so logs correlate.          | dbg_HOXmkmT9UPYlsnxqSNlEGoXL                                                                  |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `owner`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Original actor label attested by the assigned manager.                                        |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `backendTargetId`                                                                             | *string*                                                                                      | :heavy_minus_sign:                                                                            | Provider-owned target used for exact restart reconciliation.                                  |                                                                                               |
