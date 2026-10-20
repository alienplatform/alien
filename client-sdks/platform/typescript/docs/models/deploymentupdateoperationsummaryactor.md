# DeploymentUpdateOperationSummaryActor

Who took the action, when known

## Example Usage

```typescript
import { DeploymentUpdateOperationSummaryActor } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateOperationSummaryActor = {
  id: "<id>",
  kind: "user",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `email`                                                  | *string*                                                 | :heavy_minus_sign:                                       | User email when the principal is a user.                 |
| `id`                                                     | *string*                                                 | :heavy_check_mark:                                       | Stable user or service-account identifier.               |
| `kind`                                                   | [models.CauseActorKind](../models/causeactorkind.md)     | :heavy_check_mark:                                       | Type of authenticated principal that requested an event. |
| `via`                                                    | *models.DeploymentUpdateOperationSummaryViaUnion*        | :heavy_minus_sign:                                       | N/A                                                      |