# Arc

## Example Usage

```typescript
import { Arc } from "@alienplatform/platform-api/models";

let value: Arc = {
  url: "https://simple-impact.info/",
  deploymentId: "<id>",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `url`                                    | *string*                                 | :heavy_check_mark:                       | Manager URL for commands                 |
| `deploymentId`                           | *string*                                 | :heavy_check_mark:                       | Deployment ID to use in command requests |