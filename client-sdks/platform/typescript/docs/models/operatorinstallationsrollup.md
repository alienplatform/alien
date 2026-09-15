# OperatorInstallationsRollup

Rollup of this project's pull-mode installations by operations-bundle sync status, reflecting the currently enabled plugin set.

## Example Usage

```typescript
import { OperatorInstallationsRollup } from "@alienplatform/platform-api/models";

let value: OperatorInstallationsRollup = {
  total: 663003,
  ready: 369877,
  syncing: 840924,
  stuck: 429498,
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `total`                                | *number*                               | :heavy_check_mark:                     | Total pull-mode installations tracked. |
| `ready`                                | *number*                               | :heavy_check_mark:                     | N/A                                    |
| `syncing`                              | *number*                               | :heavy_check_mark:                     | N/A                                    |
| `stuck`                                | *number*                               | :heavy_check_mark:                     | N/A                                    |