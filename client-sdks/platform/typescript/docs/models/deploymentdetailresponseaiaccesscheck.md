# DeploymentDetailResponseAiAccessCheck

The setup-time answer for one declared model.

## Example Usage

```typescript
import { DeploymentDetailResponseAiAccessCheck } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseAiAccessCheck = {
  modelId: "<id>",
  resourceId: "<id>",
  result: "refused",
  testedAt: new Date("2025-04-13T03:52:07.486Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `detail`                                                                                      | *string*                                                                                      | :heavy_minus_sign:                                                                            | The cloud's error code or the reason the check could not run.                                 |
| `modelId`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | The model id as the app uses it.                                                              |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | The AI resource that declares the model.                                                      |
| `result`                                                                                      | [models.DeploymentDetailResponseResult](../models/deploymentdetailresponseresult.md)          | :heavy_check_mark:                                                                            | `verified`, `refused` (the cloud refused the model) or `not-checked`.                         |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |