# UpdateDeploymentInputsResponseHeld

## Example Usage

```typescript
import { UpdateDeploymentInputsResponseHeld } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentInputsResponseHeld = {
  digest: "<value>",
  updatedAt: new Date("2025-12-10T15:30:48.435Z"),
  updatedBy: {
    id: "<id>",
    displayName: "Don53",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `digest`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `updatedAt`                                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)          | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `updatedBy`                                                                                            | [models.UpdateDeploymentInputsResponseUpdatedBy](../models/updatedeploymentinputsresponseupdatedby.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |