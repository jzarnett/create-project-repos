use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

use gitlab::api::common::{AccessLevel, ProtectedAccessLevel, VisibilityLevel};
use gitlab::api::groups::GroupBuilder;
use gitlab::api::projects::members::AddProjectMemberBuilder;
use gitlab::api::projects::protected_branches::ProtectedAccess;
use gitlab::api::users::{CurrentUserBuilder, UsersBuilder};
use gitlab::api::{projects, Query};
use gitlab::Gitlab;

use serde::Deserialize;

const UW_GITLAB_URL: &str = "git.uwaterloo.ca";
const DEFAULT_BRANCH_NAME: &str = "main";

#[derive(Debug, Deserialize)]
struct Project {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct ProjectUser {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct CurrentUser {
    username: String,
}

#[derive(Debug, Deserialize)]
struct ProjectGroup {
    id: u64,
}

struct GitLabConfig {
    designation: String,
    group_name: String,
    template_repo: String,
    token: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 6 {
        println!(
            "Usage: {} <designation> <gitlab_group_name> <template_repo> <list_of_student_groups.csv> <token_file>",
            args.get(0).unwrap()
        );
        println!(
            "Example: {} a1 ece459-1231 ece459/ece459-a1 students.csv token.git",
            args.get(0).unwrap()
        );
        return;
    }
    let token = read_token_file(args.get(5).unwrap());
    let config = GitLabConfig {
        designation: String::from(args.get(1).unwrap()),
        group_name: String::from(args.get(2).unwrap()),
        template_repo: String::from(args.get(3).unwrap()),
        token: token.clone(),
    };

    let repo_members = parse_csv_file(args.get(4).unwrap());
    let client = Gitlab::new(String::from(UW_GITLAB_URL), token).unwrap();

    create_repos(client, repo_members, config)
}

fn create_repos(client: Gitlab, repo_members: Vec<Vec<String>>, config: GitLabConfig) {
    let current_user = find_current_user(&client);
    let import_url = format! {
        "https://{}:{}@git.uwaterloo.ca/{}.git", current_user, config.token, config.template_repo
    };
    let project_group_id = find_group_by_name(config.group_name.clone(), &client);

    for i in 0..repo_members.len() {
        let group_or_student = repo_members.get(i).unwrap();

        let project_name = if group_or_student.len() == 1 {
            format!(
                "{0}-{1}-{2}",
                config.group_name,
                config.designation,
                group_or_student.get(0).unwrap()
            )
        } else {
            format!("{}-{}-g{}", config.group_name, config.designation, (i + 1))
        };

        let gitlab_user_ids = convert_to_user_ids(&client, group_or_student);
        if gitlab_user_ids.is_empty() {
            println!("Unable to create project {project_name}; no gitlab users found");
            continue;
        }

        let project_id = create_project(
            &client,
            import_url.clone(),
            project_name.clone(),
            project_group_id,
        );
        match project_id {
            0 => {
                println!("Failed to create project {project_name}!");
                continue;
            }
            _ => println!("Created project {project_name} with id {project_id}!"),
        }
        // Sleep seems to be needed here otherwise the branch protection call gets a 404
        // I presume it's because gitlab is setting up stuff in the background but argh
        sleep(Duration::new(10, 0));

        configure_branch_protection(&client, &project_name, project_id);

        add_users_to_project(&client, gitlab_user_ids, &project_name, project_id);

        println! {"Setup of repo {project_name} is complete."}
    }
}

fn convert_to_user_ids(client: &Gitlab, group_or_student: &Vec<String>) -> Vec<u64> {
    let mut id_vector = Vec::new();
    for student in group_or_student {
        println!("Looking up student {student}...");
        let gl_user_id = retrieve_user_id(client, student);
        if let Some(id) = gl_user_id {
            println!("Student {student} has a user ID of {id}.");
            id_vector.push(id)
        }
    }
    id_vector
}

fn create_project(
    client: &Gitlab,
    import_url: String,
    project_name: String,
    project_group_id: u64,
) -> u64 {
    println!("Creating project {project_name}...");
    let project_builder = projects::CreateProjectBuilder::default()
        .namespace_id(project_group_id)
        .name(&project_name)
        .visibility(VisibilityLevel::Private)
        .default_branch(DEFAULT_BRANCH_NAME)
        .import_url(import_url)
        .build()
        .unwrap();

    let result: Result<Project, gitlab::api::ApiError<gitlab::RestError>> =
        project_builder.query(client);
    match result {
        Ok(project) => project.id,
        Err(err) => {
            println!("{}", err);
            0
        }
    }
}

fn add_users_to_project(
    client: &Gitlab,
    gitlab_user_ids: Vec<u64>,
    project_name: &String,
    project_id: u64,
) {
    println!("Adding user(s) to project {project_name}...");
    let mut added_students = 0;
    for student_gitlab_id in gitlab_user_ids {
        let member_builder = AddProjectMemberBuilder::default()
            .project(project_id)
            .access_level(AccessLevel::Developer)
            .user(student_gitlab_id)
            .build()
            .unwrap();

        // This request also returns a map but I don't really care... Danke, ich hasse es
        member_builder.query(client).unwrap_or(());
        added_students += 1;
    }
    println! {"Added {added_students} student(s) to project {project_name}."}
}

fn retrieve_user_id(client: &Gitlab, student: &String) -> Option<u64> {
    let gl_user_builder = UsersBuilder::default().username(student).build().unwrap();
    let gl_user: Vec<ProjectUser> = gl_user_builder.query(client).unwrap();
    return if gl_user.is_empty() {
        None
    } else {
        Option::from(gl_user.get(0).unwrap().id)
    };
}

fn configure_branch_protection(client: &Gitlab, project_name: &String, project_id: u64) {
    println!("Protecting default branch in project {project_name}...");
    // First, unprotect the default branch so that we are sure we set the right values
    let unprotect_branch_builder = projects::protected_branches::UnprotectBranchBuilder::default()
        .project(project_id)
        .name(DEFAULT_BRANCH_NAME)
        .build()
        .unwrap();

    // The gitlab library somehow thinks HTTP 204 is an error -_-
    unprotect_branch_builder.query(client).unwrap_or(());

    let protect_branch_builder = projects::protected_branches::ProtectBranchBuilder::default()
        .project(project_id)
        .name(DEFAULT_BRANCH_NAME)
        .allow_force_push(false)
        .allowed_to_merge(ProtectedAccess::Level(ProtectedAccessLevel::Developer))
        .allowed_to_push(ProtectedAccess::Level(ProtectedAccessLevel::Developer))
        .allowed_to_unprotect(ProtectedAccess::Level(ProtectedAccessLevel::Admin))
        .build()
        .unwrap();

    // This request returns a map but I don't really care... Danke, ich hasse es
    protect_branch_builder.query(client).unwrap_or(());
    println!("Protections applied to default branch in project {project_name}.");
}

fn find_current_user(client: &Gitlab) -> String {
    println!("Finding current user...");
    let current_user_builder = CurrentUserBuilder::default().build().unwrap();
    let curr_user: CurrentUser = current_user_builder.query(client).unwrap();
    println!("Current user is {}", curr_user.username);
    curr_user.username
}

fn find_group_by_name(group_name: String, client: &Gitlab) -> u64 {
    println!("Finding group ID for group {group_name}...");
    let group_builder = GroupBuilder::default().group(group_name).build().unwrap();

    let proj_group: ProjectGroup = group_builder.query(client).unwrap();
    proj_group.id
}

fn parse_csv_file(filename: &String) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = Vec::new();
    let lines = read_lines(filename);

    for line in lines {
        let line = line.unwrap();
        let mut inner = Vec::new();
        for user in line.split(',') {
            if !user.is_empty() {
                inner.push(String::from(user.trim()));
            }
        }
        result.push(inner);
    }
    result
}

fn read_lines(filename: &String) -> Lines<BufReader<File>> {
    let file = File::open(filename).unwrap();
    BufReader::new(file).lines()
}

fn read_token_file(filename: &String) -> String {
    let mut token = fs::read_to_string(filename)
        .unwrap_or_else(|_| panic!("Unable to read token from file {filename}"));
    token.retain(|c| !c.is_whitespace());
    token
}

#[cfg(test)]
mod tests {
    use crate::{parse_csv_file, read_token_file};
    use std::fs::{remove_file, File};
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn successfully_read_token_file() {
        let token = "1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let file_name = "tmp_token1car.git";
        {
            let mut token_file = File::create(Path::new(file_name)).unwrap();
            token_file.write_all(token.as_bytes()).unwrap();
        } // Let it go out of scope so it's closed
        let filename = String::from(file_name);
        let read_token = read_token_file(&filename);
        remove_file(Path::new(file_name)).unwrap();
        assert_eq!(read_token, token);
    }

    #[test]
    fn token_is_trimmed_nicely() {
        let token = "1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let file_name = "tmp_token2.git";
        {
            let mut token_file = File::create(Path::new(file_name)).unwrap();
            token_file.write_all("  ".as_bytes()).unwrap();
            token_file.write_all(token.as_bytes()).unwrap();
            token_file.write_all("  \n".as_bytes()).unwrap();
        } // Let it go out of scope so it's closed
        let filename = String::from(file_name);
        let read_token = read_token_file(&filename);
        remove_file(Path::new(file_name)).unwrap();
        assert_eq!(read_token, token);
    }

    #[test]
    fn can_parse_simple_csv() {
        let test_filename = String::from("test/resources/simple.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_group_csv() {
        let test_filename = String::from("test/resources/group.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        inner.push(String::from("u2sernam"));
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_group_w_spaces_csv() {
        let test_filename = String::from("test/resources/group_spaces.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        inner.push(String::from("u2sernam"));
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_multiple_csv() {
        let test_filename = String::from("test/resources/multiple.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        expected.push(inner);
        let mut inner = Vec::new();
        inner.push(String::from("u2sernam"));
        expected.push(inner);
        let mut inner = Vec::new();
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_with_newline_at_eof() {
        let test_filename = String::from("test/resources/newline_eof.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        expected.push(inner);
        let mut inner = Vec::new();
        inner.push(String::from("u2sernam"));
        expected.push(inner);
        let mut inner = Vec::new();
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_group_w_uneven_sizes_csv() {
        let test_filename = String::from("test/resources/group_uneven_sizes.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();
        let mut inner = Vec::new();
        inner.push(String::from("username"));
        inner.push(String::from("u2sernam"));
        expected.push(inner);

        let mut inner = Vec::new();
        inner.push(String::from("u3sernam"));
        inner.push(String::from("u4sernam"));
        inner.push(String::from("u5sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_mixed_csv() {
        let test_filename = String::from("test/resources/mixed.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();

        let mut inner = Vec::new();
        inner.push(String::from("username"));
        inner.push(String::from("u2sernam"));
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let mut inner = Vec::new();
        inner.push(String::from("u4sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }

    #[test]
    fn can_parse_group_w_empty_line_csv() {
        let test_filename = String::from("test/resources/group_empty_line.csv");
        let mut expected: Vec<Vec<String>> = Vec::new();

        let mut inner = Vec::new();
        inner.push(String::from("username"));
        inner.push(String::from("u2sernam"));
        inner.push(String::from("u3sernam"));
        expected.push(inner);

        let inner = Vec::new();
        expected.push(inner);

        let mut inner = Vec::new();
        inner.push(String::from("u4sernam"));
        expected.push(inner);

        let parsed = parse_csv_file(&test_filename);

        assert_eq!(parsed, expected);
    }
}
