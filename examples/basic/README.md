# Basic Example

Build `main.wasm`:

```sh
make
```

Run a Kafu service on your machine:

```sh
kafu serve --node-id cloud1 kafu-config.yaml
# open another terminal:
kafu serve --node-id edgef1 kafu-config.yaml
```

You can also run the service with `kafu singlenode`:

```sh
kafu singlenode serve kafu-config.yaml
```

Generate a Kubernetes manifest:

```sh
kafu kustomize build k8s-kafu-config.yaml
```
