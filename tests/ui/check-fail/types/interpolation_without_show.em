-- E2006: a hole calls `display`, so its value needs a `Show` instance.
data Secret = | Secret Int

reveal : Secret -> String
reveal s = "secret: \{s}"
