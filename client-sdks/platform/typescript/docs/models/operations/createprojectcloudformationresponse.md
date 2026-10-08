# CreateProjectCloudformationResponse

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { CreateProjectCloudformationResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectCloudformationResponse = {
  enabled: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `enabled`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | Whether CloudFormation package generation is enabled                 |
| `displayName`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | Human-friendly application name shown in generated install artifacts |