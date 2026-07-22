# EventListItemResponseDataCompilingCode

## Example Usage

```typescript
import { EventListItemResponseDataCompilingCode } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataCompilingCode = {
  language: "<value>",
  type: "CompilingCode",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `language`                                         | *string*                                           | :heavy_check_mark:                                 | Language being compiled (rust, typescript, etc.)   |
| `progress`                                         | *string*                                           | :heavy_minus_sign:                                 | Current progress/status line from the build output |
| `type`                                             | *"CompilingCode"*                                  | :heavy_check_mark:                                 | N/A                                                |
