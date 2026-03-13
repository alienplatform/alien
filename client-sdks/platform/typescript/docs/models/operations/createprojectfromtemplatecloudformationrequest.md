# CreateProjectFromTemplateCloudformationRequest

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { CreateProjectFromTemplateCloudformationRequest } from "@aliendotdev/platform-api/models/operations";

let value: CreateProjectFromTemplateCloudformationRequest = {
  enabled: false,
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `enabled`                                            | *boolean*                                            | :heavy_check_mark:                                   | Whether CloudFormation package generation is enabled |