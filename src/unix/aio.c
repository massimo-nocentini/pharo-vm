/* aio.c -- asynchronous file i/o
 * 
 *   Copyright (C) 1996-2006 by Ian Piumarta and other authors/contributors
 *                              listed elsewhere in this file.
 *   All rights reserved.
 *   
 *   This file is part of Unix Squeak.
 * 
 *   Permission is hereby granted, free of charge, to any person obtaining a
 *   copy of this software and associated documentation files (the "Software"),
 *   to deal in the Software without restriction, including without limitation
 *   the rights to use, copy, modify, merge, publish, distribute, sublicense,
 *   and/or sell copies of the Software, and to permit persons to whom the
 *   Software is furnished to do so, subject to the following conditions:
 * 
 *   The above copyright notice and this permission notice shall be included in
 *   all copies or substantial portions of the Software.
 * 
 *   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 *   FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 *   DEALINGS IN THE SOFTWARE.
 */

/* Authors: Ian.Piumarta@squeakland.org, eliot.miranda@gmail.com
 * 
 * Last edited: Tue Mar 29 13:06:00 PDT 2016
 */

/* Multiple changes and authors performed, use the git versioning tool to correctly address them
 */

#include "pharovm/debug.h"
#include "pharovm/semaphores/platformSemaphore.h"
#include "memoryFence.h"
#include "aio.h"

#include <sys/types.h>
#include <sys/socket.h>

#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <errno.h>
#include <signal.h>
#include <sys/types.h>
#include <sys/time.h>
#include <sys/epoll.h>
#include <sys/ioctl.h>
#include <fcntl.h>

#define INCOMING_EVENTS_SIZE	50

/*
 * This is the struct that I am keeping for the registered FD
 */
typedef struct _AioUnixDescriptor {

	sqInt fd;
	void* clientData;
	aioHandler readHandlerFn;
	aioHandler writeHandlerFn;
	struct _AioUnixDescriptor* next;
	int mask;
	int addErrno;	/* why epoll_ctl last refused fd (0 if it did not); it is logged once */

} AioUnixDescriptor;

/*
 * I have to keep a list of the registered FDs as the operations are divided in two functions
 */
AioUnixDescriptor* descriptorList = NULL;

/*
 * A handler may disable its own descriptor, or another one, while aio_handle_events
 * dispatches, and a later event of the same batch can still name that descriptor.
 * So during the dispatch a removed descriptor is not freed: it is parked in this list,
 * with fd -1 and no handlers, and freed once the dispatch is over.
 */
static AioUnixDescriptor* retiredDescriptors = NULL;
static int dispatchDepth = 0;

/*
 * I can access the elements in the list
 */
AioUnixDescriptor* AioUnixDescriptor_find(sqInt fd);
void AioUnixDescriptor_remove(sqInt fd);
void AioUnixDescriptor_removeAll();
static void AioUnixDescriptor_freeRetired();

/*
 * This is important, the AIO poll should only do a long pause if there is no pending signals for semaphores.
 * Check ExternalSemaphores to understand this function.
 */
int isPendingSemaphores();

void heartbeat_poll_enter(long microSeconds);
void heartbeat_poll_exit(long microSeconds);
static int aio_handle_events(long microSeconds);

/*
 * aioInterruptPoll sets pendingInterruption and the VM thread clears it.
 * aioInterruptPoll is reached from signal handlers (through signalSemaphoreWithIndex),
 * so no lock guards this flag: the handler could interrupt the very thread holding it.
 * It is only accessed through atomic operations instead.
 */
static int pendingInterruption = 0;
int aio_in_sleep = 0;
int aio_request_interrupt = 0;

volatile int isPooling = 0;

/* initialise asynchronous i/o */

int signal_pipe_fd[2];

void sigIOHandler(int signum){
	forceInterruptCheck();
}

void 
aioInit(void)
{
	int arg;

	if (pipe(signal_pipe_fd) != 0) {
		logErrorFromErrno("pipe");
		exit(-1);
	}

	if ((arg = fcntl(signal_pipe_fd[0], F_GETFL, 0)) < 0)
		logErrorFromErrno("fcntl(F_GETFL)");
	if (fcntl(signal_pipe_fd[0], F_SETFL, arg | O_NONBLOCK | O_ASYNC ) < 0)
		logErrorFromErrno("fcntl(F_SETFL, O_ASYNC)");

	if ((arg = fcntl(signal_pipe_fd[1], F_GETFL, 0)) < 0)
		logErrorFromErrno("fcntl(F_GETFL)");
	if (fcntl(signal_pipe_fd[1], F_SETFL, arg | O_NONBLOCK | O_ASYNC | O_APPEND) < 0)
		logErrorFromErrno("fcntl(F_SETFL, O_ASYNC)");

#if !defined(__EMSCRIPTEN__)	/* no signals there, see aioEnable() */
	signal(SIGIO, sigIOHandler);
#endif
}

/* disable handlers and close all handled non-exteral descriptors */

void 
aioFini(void)
{	
	AioUnixDescriptor_removeAll();
#if !defined(__EMSCRIPTEN__)
	signal(SIGIO, SIG_DFL);
#endif
}


/*
 * answer whether i/o becomes possible within the given number of
 * microSeconds
 */
#ifndef max
# define max(a, b)  (((a) > (b)) ? (a) : (b))
#endif


volatile int aio_requests = 0;
volatile int aio_responses = 0;

/*
 * I Try to clear all the data available in the pipe, so it does not passes the limit of data.
 */
void
aio_flush_pipe(sqInt fd){

	int bytesRead;
	char buf[1024];

	__atomic_store_n(&pendingInterruption, 0, __ATOMIC_SEQ_CST);

	do {
		bytesRead = read(fd, &buf, 1024);

		if(bytesRead == -1){

			if(errno == EAGAIN || errno == EWOULDBLOCK){
				return;
			}

			logErrorFromErrno("pipe - read");
			return;
		}

	} while(bytesRead > 0);
}

long
aioPoll(long microSeconds){
	long timeout;

	/* This consumes the pending interruption, if any, before looking at the semaphores */
	if(__atomic_exchange_n(&pendingInterruption, 0, __ATOMIC_SEQ_CST) || isPendingSemaphores()){
		timeout = 0;
	}else{
		timeout = microSeconds;
	}

	return aio_handle_events(timeout);
}

/* Answer 0, or -1 with errno set by epoll_ctl; the caller decides what to log */
static int addFDToEPoll(int epollDescriptor, sqInt fd, int events, void* userData){
	struct epoll_event ev;
	ev.events = events;
	ev.data.ptr = userData;

	return epoll_ctl(epollDescriptor, EPOLL_CTL_ADD, fd, &ev) == -1 ? -1 : 0;
}

/*
 * At most this many descriptors that epoll refused are reported in one turn;
 * the others are reported in the next turns. aio_handle_events keeps room for
 * them in its array of events.
 */
#define MAX_REFUSED_DESCRIPTORS	(INCOMING_EVENTS_SIZE / 2)

/*
 * When more descriptors are refused than one turn reports, the turns take them in
 * rotation: a turn starts with the refused descriptor at this position in
 * descriptorList (counting the refused ones only), and wraps around to the first ones.
 * Otherwise handlers that re-arm the first ones at once would starve the others.
 */
static int nextRefusedPosition = 0;

/*
 * Answer a new epoll set holding the signal pipe and every armed descriptor, or -1.
 * The descriptors epoll refuses go to refused (refusedCount of them) instead.
 */
static int fillEPollDescriptor(AioUnixDescriptor** refused, int* refusedCount){

	int epollDescriptor = epoll_create1(0);

	/* The refused descriptors before nextRefusedPosition, reported after the others */
	AioUnixDescriptor* wrapped[MAX_REFUSED_DESCRIPTORS];
	int wrappedCount = 0;
	int refusedPosition = 0;

	*refusedCount = 0;

	if (epollDescriptor == -1) {
		logErrorFromErrno("epoll_create1");
		return -1;
	}

	if(addFDToEPoll(epollDescriptor, signal_pipe_fd[0], EPOLLIN, NULL) == -1){
		logErrorFromErrno("epoll_ctl");
		logError("Error adding Pipe FD");

		if(epollDescriptor != -1){
			close(epollDescriptor);
			epollDescriptor = -1;
		}
		return -1;
	}

	AioUnixDescriptor* descriptor = descriptorList;

	while(descriptor){
		int hasRead = (descriptor->mask & AIO_R) == AIO_R;
		int hasWrite = (descriptor->mask & AIO_W) == AIO_W;
		int hasExceptions = (descriptor->mask & AIO_X) == AIO_X;

		int events = 0;
		events |= hasRead ? (EPOLLIN | EPOLLRDHUP) : 0;
		events |= hasWrite ? (EPOLLOUT | EPOLLRDHUP) : 0;
		events |= hasExceptions ? (EPOLLERR | EPOLLRDHUP) : 0;

		/*
		 * AIO handlers are one-shot: after delivery the mask is reset to 0 until
		 * the client re-arms the descriptor via aioHandle(). A zero event mask is
		 * invalid for epoll, so we must skip these inactive descriptors.
		 */
		if(events == 0){
			descriptor = descriptor->next;
			continue;
		}

		if(addFDToEPoll(epollDescriptor, descriptor->fd, events, descriptor) == -1){
			/*
			 * epoll refuses descriptors that poll(2) and select(2) accept: regular
			 * files and directories (EPERM), which are always ready, and descriptors
			 * closed underneath aio (EBADF). One of them must not take the whole set
			 * down (no notification at all, and an idle loop that never sleeps), so
			 * keep adding the others, and report this one in this turn: ready (EPERM)
			 * or failed (any other reason). Running out of memory or of epoll watches
			 * (ENOMEM, ENOSPC) is not the descriptor's fault: it is tried again next turn.
			 */
			int addErrno = errno;

			if(addErrno != descriptor->addErrno){
				if(addErrno == EPERM){
					logDebug("FD %d does not support epoll: reporting it as ready", (int) descriptor->fd);
				}else{
					logError("Error adding FD %d to Epoll", (int) descriptor->fd);
					logErrorFromErrno("epoll_ctl");
				}
				descriptor->addErrno = addErrno;
			}

			if(addErrno != ENOMEM && addErrno != ENOSPC
					&& (descriptor->readHandlerFn || descriptor->writeHandlerFn)){
				if(refusedPosition < nextRefusedPosition){
					if(wrappedCount < MAX_REFUSED_DESCRIPTORS)
						wrapped[wrappedCount++] = descriptor;
				}else if(*refusedCount < MAX_REFUSED_DESCRIPTORS){
					refused[(*refusedCount)++] = descriptor;
				}
				refusedPosition++;
			}
		}else{
			descriptor->addErrno = 0;
		}

		descriptor = descriptor->next;
	}

	/*
	 * refused holds the positions from nextRefusedPosition on, and wrapped the ones
	 * from 0: the next turn starts after the last one reported
	 */
	int wrappedTaken = 0;
	while(wrappedTaken < wrappedCount && *refusedCount < MAX_REFUSED_DESCRIPTORS){
		refused[(*refusedCount)++] = wrapped[wrappedTaken++];
	}
	nextRefusedPosition = wrappedTaken > 0 ? wrappedTaken : nextRefusedPosition + *refusedCount;
	if(nextRefusedPosition >= refusedPosition){
		nextRefusedPosition = 0;
	}

	return epollDescriptor;
}

/*
 * The events that report a descriptor epoll refused, as epoll_wait would report it
 */
static uint32_t refusedDescriptorEvents(AioUnixDescriptor* descriptor){
	uint32_t events = 0;

	if(descriptor->readHandlerFn)
		events |= EPOLLIN;
	if(descriptor->writeHandlerFn)
		events |= EPOLLOUT;
	if(descriptor->addErrno != EPERM)
		events |= EPOLLERR;

	return events;
}

static int
aio_handle_events(long microSecondsTimeout){

	struct epoll_event incomingEvents[INCOMING_EVENTS_SIZE];
	AioUnixDescriptor* refused[MAX_REFUSED_DESCRIPTORS];
	int refusedCount = 0;
	int epollReturn;
	int withError = 0;
	AioUnixDescriptor* triggeredDescriptor;

	int epollDescriptor = -1;

	long milliSecondsTimeout = microSecondsTimeout / 1000;

	//I notify the heartbeat of a pause
	heartbeat_poll_enter(microSecondsTimeout);

	sqLowLevelMFence();
	isPooling = 1;
	/*
	 * The store of isPooling must be visible before the re-check below loads the flags:
	 * this fence and the one in aioInterruptPoll make a Dekker pair with the signaller
	 * (it sets checkSignalRequests, fences, then reads isPooling). Without it the store
	 * can wait in the store buffer (x86 and arm64 both allow that) and both sides read 0.
	 */
	sqLowLevelMFence();

	epollDescriptor = fillEPollDescriptor(refused, &refusedCount);

	/*
	 * Do not wait when there are descriptors to report already, or when an interruption
	 * or a semaphore signal came after aioPoll looked: its signaller may have seen
	 * isPooling still 0, and then it did not write to the pipe.
	 */
	if(refusedCount > 0
			|| __atomic_load_n(&pendingInterruption, __ATOMIC_SEQ_CST)
			|| isPendingSemaphores()){
		milliSecondsTimeout = 0;
	}

	epollReturn = epoll_wait(epollDescriptor, incomingEvents, INCOMING_EVENTS_SIZE - refusedCount, milliSecondsTimeout);

	if(epollReturn == -1 && errno != EINTR && errno != EAGAIN){
		logErrorFromErrno("epoll_wait");
	}

	if(epollDescriptor != -1){
		close(epollDescriptor);
		epollDescriptor = -1;
	}

	sqLowLevelMFence();
	isPooling = 0;

	__atomic_store_n(&pendingInterruption, 0, __ATOMIC_SEQ_CST);

	//I notify the heartbeat of the end of the pause
	heartbeat_poll_exit(microSecondsTimeout);
	aio_flush_pipe(signal_pipe_fd[0]);

	if(epollReturn == -1){
		epollReturn = 0;
	}

	// The descriptors epoll refused are reported after the ones it answered
	for(int index = 0; index < refusedCount; index++){
		incomingEvents[epollReturn].events = refusedDescriptorEvents(refused[index]);
		incomingEvents[epollReturn].data.ptr = refused[index];
		epollReturn++;
	}

	if(epollReturn == 0){
		return 0;
	}

	dispatchDepth++;

	for(int index = 0; index < epollReturn; index++){
		//Only process the signals that are not from the interrupt pipe
		if(incomingEvents[index].data.ptr != NULL){
			triggeredDescriptor = (AioUnixDescriptor*) incomingEvents[index].data.ptr;

			// A handler disabled it earlier in this batch
			if(triggeredDescriptor->fd == -1){
				continue;
			}

			// Clearing the mask aioHandle will re add it
			triggeredDescriptor->mask = 0;
			
			if((incomingEvents[index].events & EPOLLERR) == EPOLLERR){
				withError = AIO_X;
			}else{
				withError = 0;
			}

			if((incomingEvents[index].events & EPOLLIN) == EPOLLIN){
				if(triggeredDescriptor->readHandlerFn){
					triggeredDescriptor->readHandlerFn(triggeredDescriptor->fd, triggeredDescriptor->clientData, AIO_R | withError);
				}
			}
			// The read handler may have disabled the descriptor: then its write handler is NULL
			if((incomingEvents[index].events & EPOLLOUT) == EPOLLOUT){
				if(triggeredDescriptor->writeHandlerFn){
					triggeredDescriptor->writeHandlerFn(triggeredDescriptor->fd, triggeredDescriptor->clientData, AIO_W | withError);
				}
			}
		}
	}

	if(--dispatchDepth == 0){
		AioUnixDescriptor_freeRetired();
	}

	return 1;
}

/*
 * This function is used to interrupt a aioPoll.
 * Used when signalling a Pharo semaphore to re-wake the VM and execute code of the image.
 * It is called from signal handlers too, so it takes no lock: write(2) and the atomic
 * store are async-signal-safe.
 */

void
aioInterruptPoll(){
	int n;
	int savedErrno = errno;	/* the interrupted code may be about to read it */

	sqLowLevelMFence();

	if(isPooling){
		n = write(signal_pipe_fd[1], "1", 1);
		/* A full pipe (EAGAIN) wakes the poll already */
		if(n != 1 && errno != EAGAIN && errno != EWOULDBLOCK){
			logErrorFromErrno("write to pipe");
		}
#if !defined(__EMSCRIPTEN__)	/* its pipes are in memory, with nothing to flush */
		fsync(signal_pipe_fd[1]);
#endif
	}

	__atomic_store_n(&pendingInterruption, 1, __ATOMIC_SEQ_CST);
	errno = savedErrno;
}

void 
aioEnable(sqInt fd, void *clientData, int flags)
{
	AioUnixDescriptor * descriptor;

	if (fd < 0) {
		logWarn("AioEnable(%d): IGNORED - Negative Number", fd);
		return;
	}

	descriptor = AioUnixDescriptor_find(fd);

	if(descriptor == NULL){
		descriptor = malloc(sizeof(AioUnixDescriptor));
		descriptor->readHandlerFn = NULL;
		descriptor->writeHandlerFn = NULL;
		descriptor->next = descriptorList;
		descriptorList = descriptor;
		descriptor->mask = 0;
	}

	descriptor->fd = fd;
	descriptor->clientData = clientData;
	descriptor->addErrno = 0;

	logTrace("Enabling FD: %d", (int) descriptor->fd);

	/* we should not set NBIO ourselves on external descriptors! */
	if ((flags & AIO_EXT) != AIO_EXT) {
		/*
		 * enable non-blocking asynchronous i/o and delivery of SIGIO
		 * to the active process
		 */
		int	arg;

#if defined(__EMSCRIPTEN__)
		/* there is no SIGIO (and fcntl() refuses F_SETOWN): aioPoll() alone
		 * finds out that the descriptor is ready */
		if ((arg = fcntl(fd, F_GETFL, 0)) < 0)
			logErrorFromErrno("fcntl(F_GETFL)");
		if (fcntl(fd, F_SETFL, arg | O_NONBLOCK) < 0)
			logErrorFromErrno("fcntl(F_SETFL, O_NONBLOCK)");

#elif defined(O_ASYNC)
		if (fcntl(fd, F_SETOWN, getpid()) < 0)
			logErrorFromErrno("fcntl(F_SETOWN, getpid())");
		if ((arg = fcntl(fd, F_GETFL, 0)) < 0)
			logErrorFromErrno("fcntl(F_GETFL)");
		if (fcntl(fd, F_SETFL, arg | O_NONBLOCK | O_ASYNC) < 0)
			logErrorFromErrno("fcntl(F_SETFL, O_ASYNC)");

#elif defined(FASYNC)
		if (fcntl(fd, F_SETOWN, getpid()) < 0)
			logErrorFromErrno("fcntl(F_SETOWN, getpid())");
		if ((arg = fcntl(fd, F_GETFL, 0)) < 0)
			logErrorFromErrno("fcntl(F_GETFL)");
		if (fcntl(fd, F_SETFL, arg | O_NONBLOCK | FASYNC) < 0)
			logErrorFromErrno("fcntl(F_SETFL, FASYNC)");

#elif defined(FIOASYNC)
		arg = getpid();
		if (ioctl(fd, SIOCSPGRP, &arg) < 0)
			logErrorFromErrno("ioctl(SIOCSPGRP, getpid())");
		arg = 1;
		if (ioctl(fd, FIOASYNC, &arg) < 0)
			logErrorFromErrno("ioctl(FIOASYNC, 1)");
#endif
	}
}


/* install/change the handler for a descriptor */

void 
aioHandle(sqInt fd, aioHandler handlerFn, int mask)
{
	AioUnixDescriptor *descriptor = AioUnixDescriptor_find(fd);

	if(descriptor == NULL){
		logWarn("Enabling a FD that is not present: %d - IGNORING", fd);
		return;
	}

	int hasRead = (mask & AIO_R) == AIO_R;
	int hasWrite = (mask & AIO_W) == AIO_W;

	descriptor->readHandlerFn = hasRead ? handlerFn : NULL;
	descriptor->writeHandlerFn = hasWrite ? handlerFn : NULL;
	descriptor->mask = mask;
}


/* temporarily suspend asynchronous notification for a descriptor */

void 
aioSuspend(sqInt fd, int maskToSuspend)
{
	AioUnixDescriptor *descriptor = AioUnixDescriptor_find(fd);

	if(descriptor == NULL){
		logWarn("Enabling a FD that is not present: %d - IGNORING", fd);
		return;
	}
	
	// If original MASK is 0, we don't register it before. Nothing to suspend
	if(descriptor->mask == 0){
		return;
	}
	
	int hasRead = (maskToSuspend & AIO_R) == AIO_R;
	int hasWrite = (maskToSuspend & AIO_W) == AIO_W;
	int hasExceptions = (maskToSuspend & AIO_X) == AIO_X;

	if(hasRead){
		descriptor->readHandlerFn = NULL;
		descriptor->mask &= ~AIO_R;
	}

	if(hasWrite){
		descriptor->writeHandlerFn = NULL;		
		descriptor->mask &= ~AIO_W;
	}

	if(hasExceptions){
		descriptor->mask &= ~AIO_X;
	}
}


/* definitively disable asynchronous notification for a descriptor */

void 
aioDisable(sqInt fd)
{
	AioUnixDescriptor_remove(fd);
}

AioUnixDescriptor* AioUnixDescriptor_find(sqInt fd){
	AioUnixDescriptor* found;

	found = descriptorList;
	while(found != NULL){
		if(found->fd == fd)
			return found;
		found = found->next;
	}

	return NULL;
}

/*
 * Free a descriptor that is no longer in descriptorList, or park it until the
 * dispatch in progress is over (see retiredDescriptors)
 */
static void AioUnixDescriptor_retire(AioUnixDescriptor* descriptor){

	if(dispatchDepth == 0){
		free(descriptor);
		return;
	}

	descriptor->fd = -1;
	descriptor->readHandlerFn = NULL;
	descriptor->writeHandlerFn = NULL;
	descriptor->mask = 0;
	descriptor->next = retiredDescriptors;
	retiredDescriptors = descriptor;
}

static void AioUnixDescriptor_freeRetired(){
	AioUnixDescriptor* current;

	while(retiredDescriptors != NULL){
		current = retiredDescriptors;
		retiredDescriptors = current->next;
		free(current);
	}
}

void AioUnixDescriptor_remove(sqInt fd){
	AioUnixDescriptor* found;
	AioUnixDescriptor* prev = NULL;

	found = descriptorList;

	while(found != NULL){

		if(found->fd == fd){
			if(descriptorList == found){
				descriptorList = found->next;
			}else{
				prev->next = found->next;
			}
			AioUnixDescriptor_retire(found);
			return;
		}
		prev = found;
		found = found->next;
	}

}

void AioUnixDescriptor_removeAll(){
	AioUnixDescriptor* current;

	while(descriptorList != NULL){
		current = descriptorList;
		descriptorList = current->next;
		AioUnixDescriptor_retire(current);
	}
}