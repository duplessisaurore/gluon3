# Contributing

If you're interested in helping build `Gluon3` and all associated components, thank you. 

If you haven't already, please join the [Discord](https://discord.gg/wXzj2cqZ3Q) for discussion about anything `Gluon3` related (or just for fun!).

## Committing

Always write a clear message for your commits. One-line messages are fine for small changes, but bigger changes should look like this:

    $ git commit -m "A brief summary of the commit
    > 
    > A paragraph describing what changed and its impact."

There is no necessary style for commits. Feel free to use conventional commits or anything as long as the message is descriptive enough.

Make sure to follow the pull request template for all PRs!

## Guidelines

Please adhere to the following general guidelines:
1. Always use granular imports instead of blob imports like ``crate::foo::*``
2. Ensure that all `///` doc comments are placed above `#[derive(Trait)]` usages.
3. `//` comments should always be placed ABOVE the line they're commenting, never in-line unless necessary!
4. Avoid `/* */` comments.
5. Whenever an identifier is referred to use \`identifier\` code blocks to signify that.
6. Make sure to format your code using `rustfmt` and check for issues with `clippy` before any commit!

## AI Policy

Direct usage of generative AI (large language models or similar) for contributions is not allowed unless explicitly stated otherwise in this document. This includes contributions to both code and non-code assets.

We recognize that English may not be the primarily language for all contributors and that machine translation is an indispensable tool for proper collaboration. Therefore machine translation is not subject to the above policy.

Any contributor found to have submitted contributions with the usage of AI may be subject to:

- Blanket rejection of all future contributions to Gluon3.
- Retroactive removal of any potentially suspect AI-generated code and asset contributions.
- Further Code of Conduct actions if these contributions were found to be submitted in bad faith.

Thanks,
Gluon3 Contributors 💛