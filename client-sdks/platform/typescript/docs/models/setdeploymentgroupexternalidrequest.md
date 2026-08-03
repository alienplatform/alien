# SetDeploymentGroupExternalIdRequest

## Example Usage

```typescript
import { SetDeploymentGroupExternalIdRequest } from "@alienplatform/platform-api/models";

let value: SetDeploymentGroupExternalIdRequest = {
  externalId: "ext_example_01",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       | Example                                                                           |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `externalId`                                                                      | *string*                                                                          | :heavy_check_mark:                                                                | Case-sensitive, URL- and header-safe identifier from the integrating application. | ext_example_01                                                                    |