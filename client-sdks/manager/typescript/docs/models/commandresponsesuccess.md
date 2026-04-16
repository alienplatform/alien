# CommandResponseSuccess

Command executed successfully

## Example Usage

```typescript
import { CommandResponseSuccess } from "@alienplatform/manager-api/models";

let value: CommandResponseSuccess = {
  response: {
    inlineBase64: "<value>",
    mode: "inline",
  },
  status: "success",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `response`                                             | *models.BodySpec*                                      | :heavy_check_mark:                                     | Body specification supporting inline and storage modes |
| `status`                                               | *"success"*                                            | :heavy_check_mark:                                     | N/A                                                    |