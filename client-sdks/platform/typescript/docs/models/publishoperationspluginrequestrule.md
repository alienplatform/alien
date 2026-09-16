# PublishOperationsPluginRequestRule

## Example Usage

```typescript
import { PublishOperationsPluginRequestRule } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginRequestRule = {
  apiGroup: "<value>",
  resource: "<value>",
  verbs: [
    "patch",
  ],
  reason: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `apiGroup`                                                                                     | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `resource`                                                                                     | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `verbs`                                                                                        | [models.PublishOperationsPluginRequestVerb](../models/publishoperationspluginrequestverb.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `resourceNames`                                                                                | *string*[]                                                                                     | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `reason`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |