# SetFirstPartyDeploymentInputsRequest

## Example Usage

```typescript
import { SetFirstPartyDeploymentInputsRequest } from "@alienplatform/platform-api/models";

let value: SetFirstPartyDeploymentInputsRequest = {
  platform: "gcp",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                       | [models.SetFirstPartyDeploymentInputsRequestPlatform](../models/setfirstpartydeploymentinputsrequestplatform.md) | :heavy_check_mark:                                                                                               | Represents the target cloud platform.                                                                            |
| `inputValues`                                                                                                    | Record<string, *models.StackInputValueRequest*>                                                                  | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |