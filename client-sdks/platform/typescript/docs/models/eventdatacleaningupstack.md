# EventDataCleaningUpStack

## Example Usage

```typescript
import { EventDataCleaningUpStack } from "@alienplatform/platform-api/models";

let value: EventDataCleaningUpStack = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpStack",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `stackName`                                            | *string*                                               | :heavy_check_mark:                                     | Name of the stack being cleaned up                     |
| `strategyName`                                         | *string*                                               | :heavy_check_mark:                                     | Name of the deployment strategy being used for cleanup |
| `type`                                                 | *"CleaningUpStack"*                                    | :heavy_check_mark:                                     | N/A                                                    |
