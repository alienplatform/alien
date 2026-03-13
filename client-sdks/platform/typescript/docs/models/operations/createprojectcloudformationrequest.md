# CreateProjectCloudformationRequest

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { CreateProjectCloudformationRequest } from "@aliendotdev/platform-api/models/operations";

let value: CreateProjectCloudformationRequest = {
  enabled: false,
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `enabled`                                            | *boolean*                                            | :heavy_check_mark:                                   | Whether CloudFormation package generation is enabled |