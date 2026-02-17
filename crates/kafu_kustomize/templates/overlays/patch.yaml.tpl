apiVersion: v1
kind: Pod
metadata:
  name: kafu-server
spec:
  nodeSelector:
    kafu-node: {placement}
  containers:
  - name: kafu-server
    command:
    - kafu_serve
    - --node-id
    - "{node_id}"
    - /etc/kafu/kafu-config.yaml
