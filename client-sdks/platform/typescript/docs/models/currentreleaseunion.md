# CurrentReleaseUnion


## Supported Types

### `models.CurrentRelease`

```typescript
const value: models.CurrentRelease = {
  stack: {
    id: "<id>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        dependencies: [
          {
            id: "<id>",
            type: "<value>",
          },
        ],
        lifecycle: "live",
      },
    },
  },
};
```

### `string`

```typescript
const value: string = "<value>";
```

