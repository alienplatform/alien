# ListOperationsPluginsRequest

## Example Usage

```typescript
import { ListOperationsPluginsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListOperationsPluginsRequest = {
  project: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `project`                                                              | *string*                                                               | :heavy_check_mark:                                                     | Filter by project ID or name.                                          |
| `deployment`                                                           | *string*                                                               | :heavy_minus_sign:                                                     | Only the plugins this deployment declares, at their declared versions. |
