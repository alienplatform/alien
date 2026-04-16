# ProjectListItemResponseCloudformation

CloudFormation package configuration. If null, CloudFormation packages will not be generated.

## Example Usage

```typescript
import { ProjectListItemResponseCloudformation } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseCloudformation = {
  enabled: true,
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `enabled`                                            | *boolean*                                            | :heavy_check_mark:                                   | Whether CloudFormation package generation is enabled |