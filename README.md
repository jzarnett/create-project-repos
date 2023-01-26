# Project Repo Creator

Here's a small tool that I wrote to help me set up repos in the UW gitlab in an efficient way. 

Expectations: 

1. You have created a gitlab group (e.g., `ece459-1231`) that will contain all the repos. You can add other instructors, lab instructors, and marking TAs as members of that group so you don't have to add them to each repo and you'll have access as needed.

2. You have a template repo with the starter code (or at least starter repo configuration) that you want to distribute to everyone. A basically-empty repo is fine, just throw a README in there if you must.

3. Your file of students and groups to create is prepared (correctly); see the "Usage" section for what to do.

Output:

The tool creates git repositories in the designated gitlab group that contain the starter code pointed to by the template repository.

The group members or student specified in the file are (is) added as direct member(s) of the project with Developer permissions -- it allows them to do work but not to mess with the setup in a way that would be bad for marking.

The repos are created with branch protections enabled that prevent force pushes. This is important, because without this, a student could have a commit before a deadline, do more work after the deadline, and rewrite history to make it appear that the work was done before the deadline in the commit history.

## Usage
I tried to make it easy but there are a few things that could not be avoided. It takes five commandline arguments (order and format matter, sadly).

Formally:
```
executable <designation> <gitlab_group_name> <template_repo> <list_of_students_or_groups.csv> <token_file>
```

In order then:
### `designation`
The designation refers to how this repo should be designated: typically assignment 1 would be given as `a1`, but you could say it's the final exam by putting `final`, or a project by `p`. Your call.

### `gitlab_group_name`
This is the group in gitlab where you want the repos to be created. This parameter is also used in building the name of the repo (see below). It's not so much that it is important for the student -- they probably only take the course once -- but it really helps course staff if names are clear even without looking at the group the project is in. So if the course and term I'm running this in are ECE 459 and 1231 (Winter 2023), I would choose `ece459-1231`.

### `template_repo`
This is where the starter code is found for the repo. The repo obviously must exist and it's formed as `group/repo` like `ece459-starter-code/ece459-a1`. The repo there is used as the basis and it includes files as well as authors/commit history, so be mindful that if you had a (partial) solution in there at some point and deleted things to make it the starter code, that history is visible.

### `list_of_students_or_groups.csv`
Provide the filename of a CSV (comma-separated-value) format file that contains information about the students and groups to be created. The only content here is student usernames (e.g., jzarnett for me). You have two options about what to do here (and can mix and match):

If there is EXACTLY one username on a line, the repo will be created as `group-desigation-username`, so `ece459-1231-a1-jzarnett`. That username will be added as a member of the project.

If there are MULTIPLE usernames on a line, the repo will be created as `group-designation-g{linenumber+1}`. So if this is the 8th line of the CSV file it will be created as `ece459-1231-proj-g9`. All usernames on that line will be added as members of the project. 

### `token_file`
A plain text file containing your gitlab user token. Whoever the token belongs to is the owner of the repos that we are creating here. No newline or anything at the end of the file.


## TODOs
- Right now I have a `sleep` call in between creating the repo and trying to set the branch protection rules. That's annoying, but I wasn't easily able to solve it in the first version.
- This isn't parallelized, though in practice I'd like to try doing 2-3 repos at once. Helps when there's 400+ students.
- Groups are always assigned sequential numbers based on their order in the file. In the future, maybe support a prefix so the group number could be something else, like "201.1" if the groups are subdivided in some way (e.g., by lab section).
- Letting you give params in any order might be good.
- Some repo might use `master` instead of `main` as the name of the default branch. It would be cool to handle that automatically.
- When I get out of developer prison for not writing tests, I expect I will have to write tests as restitution.
- Right now we'll fail the whole process if a student doesn't have their gitlab account set up. We should catch the panic and just write down their name and go on. Ideally before making their repo.

## Changelog

### 1.0.0
Initial, non-parallelized version with ugly sleep call but it does work.
