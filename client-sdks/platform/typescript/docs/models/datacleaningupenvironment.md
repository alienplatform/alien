# DataCleaningUpEnvironment

## Example Usage

```typescript
import { DataCleaningUpEnvironment } from "@alienplatform/platform-api/models";

let value: DataCleaningUpEnvironment = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpEnvironment",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `stackName`                                            | *string*                                               | :heavy_check_mark:                                     | Name of the stack being cleaned up                     |
| `strategyName`                                         | *string*                                               | :heavy_check_mark:                                     | Name of the deployment strategy being used for cleanup |
| `type`                                                 | *"CleaningUpEnvironment"*                              | :heavy_check_mark:                                     | N/A                                                    |