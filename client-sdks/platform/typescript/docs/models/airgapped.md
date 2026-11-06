# Airgapped

## Example Usage

```typescript
import { Airgapped } from "@alienplatform/platform-api/models";

let value: Airgapped = {
  bundleSigningKey: "<value>",
  command: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `bundleSigningKey`                                                     | *string*                                                               | :heavy_check_mark:                                                     | Deliver this trusted signing key separately to the site administrator. |
| `command`                                                              | *string*                                                               | :heavy_check_mark:                                                     | First command to run on the online machine.                            |