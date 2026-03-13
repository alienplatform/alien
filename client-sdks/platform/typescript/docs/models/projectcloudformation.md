# ProjectCloudformation

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { ProjectCloudformation } from "@aliendotdev/platform-api/models";

let value: ProjectCloudformation = {
  enabled: false,
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `enabled`                                            | *boolean*                                            | :heavy_check_mark:                                   | Whether CloudFormation package generation is enabled |