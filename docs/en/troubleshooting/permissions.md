# Permission / Root troubleshooting

Check actual UID first: ADB shell is usually 2000, Root is 0, and Termux has an app UID. Ordinary Termux can access its files and model tasks but lacks shell/root UI injection, screenshot, and companion Binder permissions.

For `permission denied`, inspect target paths, ownership, modes, SELinux, and service permissions; failed reads do not prove absence. `execute_user_mode = "normal"` does not invoke su. Failed su in root mode refuses rather than falling back. Failed host root adbd does not prove device su is unavailable.

Private configuration uses 0600. Launchers fail early on identity mismatch; do not make key files globally readable. Redeploy explicit config under the intended identity instead.

The companion requires manually enabled Accessibility/keyboard services, sometimes after allowing restricted settings. Services are independent; Unicode replace requires IME. Service loss after approval, partial trees, and package/identity changes refuse actions; retain these checks.

See [Root](../guide/root.md), [Android Bridge](../advanced/android-bridge.md), and [security](../reference/security-model.md).
