# SyncReleaseRequest

Request to release deployment lock

## Example Usage

```typescript
import { SyncReleaseRequest } from "@alienplatform/platform-api/models";

let value: SyncReleaseRequest = {
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  session: "<value>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   | Example                       |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `deploymentId`                | *string*                      | :heavy_check_mark:            | Deployment ID to release      | ag_pnj2da55wi5sxbdcav9t273je  |
| `session`                     | *string*                      | :heavy_check_mark:            | Session identifier to release |                               |