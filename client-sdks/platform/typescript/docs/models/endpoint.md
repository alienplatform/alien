# Endpoint

## Example Usage

```typescript
import { Endpoint } from "@alienplatform/platform-api/models";

let value: Endpoint = {
  url: "https://worse-fold.org",
  source: "runtime",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `url`                                                                            | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | [models.ContainerRegistryStateSource](../models/containerregistrystatesource.md) | :heavy_check_mark:                                                               | N/A                                                                              |