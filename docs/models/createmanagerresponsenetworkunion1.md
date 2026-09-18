# CreateManagerResponseNetworkUnion1


## Supported Types

### `models.CreateManagerResponseNetworkUseDefault1`

```typescript
const value: models.CreateManagerResponseNetworkUseDefault1 = {
  type: "use-default",
};
```

### `models.CreateManagerResponseNetworkCreate1`

```typescript
const value: models.CreateManagerResponseNetworkCreate1 = {
  type: "create",
};
```

### `models.CreateManagerResponseNetworkByoVpcAws1`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcAws1 = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.CreateManagerResponseNetworkByoVpcGcp1`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcGcp1 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.CreateManagerResponseNetworkByoVnetAzure1`

```typescript
const value: models.CreateManagerResponseNetworkByoVnetAzure1 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

