`@TEMPLATE` keeps a count. Press Reset on its page to start it again.
`@TEMPLATE.count` reads it, and `@TEMPLATE.armed` is a switch a script
can set.

## Counting

`add` changes the count. The reset signal carries
the count it ended at.

```
set @shown to @TEMPLATE.count
set @TEMPLATE.armed to true
run @TEMPLATE.add(5)
on @TEMPLATE.reset > 10 {
    set @Cheer to true
}
```
