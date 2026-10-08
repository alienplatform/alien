# UpdateDeploymentInputsRequest

## Example Usage

```typescript
import { UpdateDeploymentInputsRequest } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentInputsRequest = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `expectedBaseOperationId`                                                                                                      | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Save only if this is still the latest accepted deployment operation. Follow the returned operation ID before continuing setup. |
| `inputValues`                                                                                                                  | Record<string, *models.StackInputValueRequest*>                                                                                | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
| `clearInputIds`                                                                                                                | *string*[]                                                                                                                     | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
