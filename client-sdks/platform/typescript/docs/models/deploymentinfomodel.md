# DeploymentInfoModel

## Example Usage

```typescript
import { DeploymentInfoModel } from "@alienplatform/platform-api/models";

let value: DeploymentInfoModel = {
  accessTest: "verified",
  availability: "available",
  blockers: [
    "model-activation-required",
  ],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [models.DeploymentInfoAccessTest](../models/deploymentinfoaccesstest.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.DeploymentInfoAvailabilityEnum](../models/deploymentinfoavailabilityenum.md)          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.DeploymentInfoBlocker](../models/deploymentinfoblocker.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.DeploymentInfoClientApi](../models/deploymentinfoclientapi.md)[]                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |