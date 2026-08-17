# OperationsPolicyRule

## Example Usage

```typescript
import { OperationsPolicyRule } from "@alienplatform/platform-api/models";

let value: OperationsPolicyRule = {
  pattern: "<value>",
  decision: "manual",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `pattern`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | `plugin/operation`, `plugin/*`, or `*`.                                          |
| `decision`                                                                       | [models.OperationsPolicyRuleDecision](../models/operationspolicyruledecision.md) | :heavy_check_mark:                                                               | auto: run immediately. manual: needs customer approval before running.           |