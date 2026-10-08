# PackagesConfigPatchWorkloadReadAccess

## Example Usage

```typescript
import { PackagesConfigPatchWorkloadReadAccess } from "@alienplatform/platform-api/models";

let value: PackagesConfigPatchWorkloadReadAccess = {
  rules: [
    {
      apiGroup: "<value>",
      resources: [
        "<value 1>",
      ],
    },
  ],
  serviceAccountProfile: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `rules`                                                                  | [models.PackagesConfigPatchRule](../models/packagesconfigpatchrule.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `serviceAccountProfile`                                                  | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |