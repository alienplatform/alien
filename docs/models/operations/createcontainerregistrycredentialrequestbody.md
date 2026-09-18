# CreateContainerRegistryCredentialRequestBody

## Example Usage

```typescript
import { CreateContainerRegistryCredentialRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateContainerRegistryCredentialRequestBody = {
  label: "<value>",
  scope: "pushPull",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `label`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `scope`                                                                                       | [operations.Scope](../../models/operations/scope.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `repositorySubset`                                                                            | *string*[]                                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |