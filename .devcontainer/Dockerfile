FROM rust:bullseye AS development

ENV DEBIAN_FRONTEND=noninteractive

COPY ps1.bash .
RUN cat ps1.bash >> ~/.bashrc && rm ps1.bash

COPY .bash_aliases /home/vscode/.bash_aliases

# Enable our git hooks and set the permisisons on docker sock.



RUN apt-get update && apt-get install sudo lsb-release git -y  && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -r /var/cache/* /var/lib/apt/lists/*

RUN sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list' \
   && wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | apt-key add - \
   && apt-get -y update \
   && apt-get -y install postgresql-client \
   && apt-get autoremove -y && apt-get clean -y

ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID


RUN groupadd --gid $USER_GID $USERNAME \
    && useradd -s /bin/bash --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME\
    && chmod 0440 /etc/sudoers.d/$USERNAME 
    
USER $USERNAME

RUN cargo install sqlx-cli --no-default-features --features native-tls,postgres
RUN rustup component add clippy

RUN sudo mkdir -p /workspace/target && \
 sudo chown $USERNAME:$USERNAME /workspace && \
 sudo chown -hR $USERNAME:$USERNAME /workspace/* && \
 sudo chown -hR $USERNAME:$USERNAME /home/vscode

RUN sudo echo 'git config core.hooksPath /vscode/.devcontainer/.githooks' >> ~/.bashrc