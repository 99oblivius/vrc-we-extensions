`@TEMPLATE` keeps a count. Press Reset on its page to start it again.

## Counting

`add` changes the count. The reset signal carries
the count it ended at.

```
run @TEMPLATE.add(5)
on @TEMPLATE.reset > 10 {
    set @Cheer to true
}
```
