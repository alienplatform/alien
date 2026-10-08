# GetPendingWorkspaceInvitationResponse

An active invitation addressed to the signed-in user for this workspace, if any.

## Example Usage

```typescript
import { GetPendingWorkspaceInvitationResponse } from "@alienplatform/platform-api/models/operations";

let value: GetPendingWorkspaceInvitationResponse = {
  invitationId: "winv_DsgltMIFV0GmqtxV5NYTtrknrna",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     | Example                                         |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `invitationId`                                  | *string*                                        | :heavy_check_mark:                              | Unique identifier for the workspace invitation. | winv_DsgltMIFV0GmqtxV5NYTtrknrna                |