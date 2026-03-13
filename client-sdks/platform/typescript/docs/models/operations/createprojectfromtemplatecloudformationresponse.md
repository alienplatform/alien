# CreateProjectFromTemplateCloudformationResponse

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { CreateProjectFromTemplateCloudformationResponse } from "@aliendotdev/platform-api/models/operations";

let value: CreateProjectFromTemplateCloudformationResponse = {
  enabled: true,
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `enabled`                                            | *boolean*                                            | :heavy_check_mark:                                   | Whether CloudFormation package generation is enabled |