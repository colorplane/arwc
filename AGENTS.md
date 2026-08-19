# Agent notes

## Notarization

Never wait for Apple notarization (`notarytool submit --wait`, polling `notarytool info`, or sleeping until Accepted). Submit if needed, print the submission id, then give the user the commands to check status themselves.

After a submit:

```
xcrun notarytool info SUBMISSION_ID --keychain-profile colorplane-notary
```

All recent submissions:

```
xcrun notarytool history --keychain-profile colorplane-notary
```

Do not pass `--wait` or `--timeout` to `notarytool submit`.

Remove the stored Keychain profile (`colorplane-notary`):

```
security delete-generic-password -s "com.apple.gke.notary.tool" -a "colorplane-notary"
```
