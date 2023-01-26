use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

use csv::ReaderBuilder;
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
                "{}-{}-{}",
                config.group_name,
                config.designation,
                group_or_student.get(0).unwrap()
            )
        } else {
            format!("{}-{}-g{}", config.group_name, config.designation, (i + 1))
        };

        let gitlab_user_ids = convert_to_user_ids(&client, group_or_student);
        if gitlab_user_ids.is_empty() {
            println!(
                "Unable to create project {}; no gitlab users found",
                project_name
            );
            continue;
        }

        let project = create_project(
            &client,
            import_url.clone(),
            project_name.clone(),
            project_group_id,
        );
        println!("Created project {} with id {}!", project_name, project.id);
        // Sleep seems to be needed here otherwise the branch protection call gets a 404
        // I presume it's because gitlab is setting up stuff in the background but argh
        sleep(Duration::new(10, 0));

        configure_branch_protection(&client, &project_name, project.id);

        add_users_to_project(&client, gitlab_user_ids, &project_name, project.id);

        println! {"Setup of repo {} is complete.", project_name}
    }
}

fn convert_to_user_ids(client: &Gitlab, group_or_student: &Vec<String>) -> Vec<u64> {
    let mut id_vector = Vec::new();
    for student in group_or_student {
        println!("Looking up student {}...", student);
        let gl_user_id = retrieve_user_id(client, student);
        if let Some(id) = gl_user_id {
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
) -> Project {
    println!("Creating project {}...", project_name);
    let project_builder = projects::CreateProjectBuilder::default()
        .namespace_id(project_group_id)
        .name(&project_name)
        .visibility(VisibilityLevel::Private)
        .default_branch(DEFAULT_BRANCH_NAME)
        .import_url(import_url)
        .build()
        .unwrap();

    let project: Project = project_builder.query(client).unwrap();
    project
}

fn add_users_to_project(
    client: &Gitlab,
    gitlab_user_ids: Vec<u64>,
    project_name: &String,
    project_id: u64,
) {
    println!("Adding user(s) to project {}...", project_name);
    for student_gitlab_id in gitlab_user_ids {
        let member_builder = AddProjectMemberBuilder::default()
            .project(project_id)
            .access_level(AccessLevel::Developer)
            .user(student_gitlab_id)
            .build()
            .unwrap();

        // This request also returns a map but I don't really care... Danke, ich hasse es
        member_builder.query(client).unwrap_or(());
    }
}

fn retrieve_user_id(client: &Gitlab, student: &String) -> Option<u64> {
    let gl_user_builder = UsersBuilder::default().search(student).build().unwrap();
    let gl_user: Vec<ProjectUser> = gl_user_builder.query(client).unwrap();
    return if gl_user.is_empty() {
        None
    } else {
        Option::from(gl_user.get(0).unwrap().id)
    };
}

fn configure_branch_protection(client: &Gitlab, project_name: &String, project_id: u64) {
    println!("Protecting default branch in project {}...", project_name);
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
}

fn find_current_user(client: &Gitlab) -> String {
    println!("Finding current user...");
    let current_user_builder = CurrentUserBuilder::default().build().unwrap();
    let curr_user: CurrentUser = current_user_builder.query(client).unwrap();
    curr_user.username
}

fn find_group_by_name(group_name: String, client: &Gitlab) -> u64 {
    println!("Finding group ID for group {}...", group_name);
    let group_builder = GroupBuilder::default().group(group_name).build().unwrap();

    let proj_group: ProjectGroup = group_builder.query(client).unwrap();
    proj_group.id
}

fn parse_csv_file(filename: &String) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = Vec::new();
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .from_path(filename)
        .unwrap();

    for line in rdr.records() {
        let line = line.unwrap();
        let mut inner = Vec::new();
        for user in line.iter() {
            inner.push(String::from(user))
        }
        result.push(inner);
    }
    result
}

fn read_token_file(filename: &String) -> String {
    fs::read_to_string(filename)
        .unwrap_or_else(|_| panic!("Unable to read token from file {}", filename))
}
