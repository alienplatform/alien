# SyncReleaseRequest

Request to release deployment lock

## Example Usage

```typescript
import { SyncReleaseRequest } from "@alienplatform/platform-api/models";

let value: SyncReleaseRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  session: "<value>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   | Example                       |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `deploymentId`                | *string*                      | :heavy_check_mark:            | Deployment ID to release      | dep_0c29fq4a2yjb7kx3smwdgxlc  |
| `session`                     | *string*                      | :heavy_check_mark:            | Session identifier to release |                               |