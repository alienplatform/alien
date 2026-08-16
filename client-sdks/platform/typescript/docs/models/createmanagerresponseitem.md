# CreateManagerResponseItem

## Example Usage

```typescript
import { CreateManagerResponseItem } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseItem = {
  item: "deployment",
  source: {
    type: "project-release",
    releaseChannel: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
  required: false,
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `item`                                                                                       | [models.CreateManagerResponseItemEnum](../models/createmanagerresponseitemenum.md)           | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *models.CreateManagerResponseSourceUnion*                                                    | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `required`                                                                                   | *boolean*                                                                                    | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `configuration`                                                                              | [models.CreateManagerResponseConfiguration](../models/createmanagerresponseconfiguration.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |