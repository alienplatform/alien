# DeploymentInputsResponseHeld

## Example Usage

```typescript
import { DeploymentInputsResponseHeld } from "@alienplatform/platform-api/models";

let value: DeploymentInputsResponseHeld = {
  digest: "<value>",
  updatedAt: new Date("2025-08-05T22:49:35.992Z"),
  updatedBy: {
    id: "<id>",
    displayName: "Bessie_Hartmann47",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedBy`                                                                                   | [models.DeploymentInputsResponseUpdatedBy](../models/deploymentinputsresponseupdatedby.md)    | :heavy_check_mark:                                                                            | N/A                                                                                           |