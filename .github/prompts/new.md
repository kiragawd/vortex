# TODO

## The UI doesn't show the lines for complex 100 dag but shows all the nodes , on clicking exapand all it shows the reamining lines to the nodes as well , it should only show the first 25 nodes and lines , each node should be clickable to see what downstream nodes are there on the dag and when I click on expand all I should be able to see the entire DAG.

## Use the browser tool to take screenshots of the Entire UI to understand the Above and fix the bugs, Once you fix the bugs make sure to take screenshot of everything again to verify.


## Use the browser tool to take screenshots of the Entire UI to understand if there are any more bugs ,Proceed to fix the bugs, Once you fix the bugs make sure to take screenshot of everything again to verify.

## There are many sql files in the Migration , i think we consolidate them into 2-3 sql files depenedent on how many tables and values we need.

## The current snowflake connector is written as experimental finish the connector if there is anything left to build after that add an option to use snowsql sdk to connect to snowflake directly instead of using snowflake api as this will ensure ours is true enterprise orchestrator. If we don't have an option to use snowflake keypair to authenticate to snowflake add it 


## The repos doesn't has a lot of node modules and pychahce files from the runs delete all of them so that there is no clutter and the repo is minimal when I push to GIT 

## we have added many enterprise features but I am not sure we have added cli commands for all of them , go ahead and add CLI commands for all of them

## there are several warnings while compiling vortex if possible resolve all of the warnings , if you need help/decision in resolving them let me know 


## Do a Full passthrough of the entire codebase with a finetooth comb make sure there are no todo left in the code , if there are finish building those todo's. Make sure vortex is ready for enterprise deployment.

## Once all of the above is done Test creating a docker image with swarm and run the docker image to make sure everything is working as expected so that it is easy for others to test vortex, use some other folder for deployment of docker image since I don't want to clutter the repository , something like the downloads folder 

## After deployment of docker write tests for all functionality in vortex if already doesn't exist and test using the docker image.

